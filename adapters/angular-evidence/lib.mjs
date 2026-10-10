// wicked-estate Angular compiler adapter (TS-S3): compiler-resolved template input bindings →
// SemanticEvidence v1 (docs/ENGINE-CONTRACT.md §3.5). Translation only; the engine correlates and
// projects. Everything the compiler did not resolve is reported, never guessed.

import path from 'node:path';
import { createRequire } from 'node:module';
import ts from 'typescript';
import * as ng from '@angular/compiler';
import { NgtscProgram, readConfiguration, VERSION } from '@angular/compiler-cli';

export const ADAPTER_VERSION = '0.1.0';
export const PRODUCER = 'angular-compiler-adapter';
/** The one Angular major this adapter is validated against (its APIs are not semver-stable). */
export const SUPPORTED_MAJOR = 22;

/** Exit code for an unsupported or unreadable Angular version: no envelope is written. */
export const EXIT_UNSUPPORTED = 2;

export class UnsupportedVersion extends Error {}

/** A repository-relative, `/`-separated document path, or `null` outside `root`. */
export function toDocPath(absolute, root, p = path) {
  const rel = p.relative(root, absolute);
  if (!rel || rel.startsWith('..') || p.isAbsolute(rel)) return null;
  return rel.split(p.sep).join('/').replace(/\\/g, '/');
}

function majorOf(version) {
  const m = /^(\d+)\.\d+\.\d+/.exec(String(version ?? ''));
  return m ? Number(m[1]) : null;
}

/** Refuse anything but the validated major, for both the compiler and the project's core. */
export function checkVersions(projectDir) {
  const compilerMajor = majorOf(VERSION.full);
  if (compilerMajor !== SUPPORTED_MAJOR) {
    throw new UnsupportedVersion(`@angular/compiler-cli ${VERSION.full} is not supported (validated: ${SUPPORTED_MAJOR}.x)`);
  }
  let coreVersion;
  try {
    const req = createRequire(path.join(projectDir, 'package.json'));
    coreVersion = req('@angular/core/package.json').version;
  } catch {
    throw new UnsupportedVersion(`cannot resolve @angular/core from ${projectDir}`);
  }
  const coreMajor = majorOf(coreVersion);
  if (coreMajor === null) throw new UnsupportedVersion(`@angular/core version ${JSON.stringify(coreVersion)} is malformed`);
  if (coreMajor !== SUPPORTED_MAJOR) {
    throw new UnsupportedVersion(`@angular/core ${coreVersion} is not supported (validated: ${SUPPORTED_MAJOR}.x)`);
  }
  return { compiler: VERSION.full, core: coreVersion };
}

// `current` (ImplicitReceiver) and `this.current` (ThisReceiver, which does NOT extend it).
const isHostReceiver = (r) => r instanceof ng.ImplicitReceiver || r instanceof ng.ThisReceiver;

/** Top-level reads of a bound expression, and what could not be tied to a member. */
function collectReads(ast, reads, unresolved) {
  if (!ast || typeof ast !== 'object') return;
  if (ast instanceof ng.ASTWithSource) return collectReads(ast.ast, reads, unresolved);
  if (ast instanceof ng.PropertyRead || ast instanceof ng.SafePropertyRead) {
    if (isHostReceiver(ast.receiver)) reads.push(ast);
    else collectReads(ast.receiver, reads, unresolved);
    return;
  }
  if (ast instanceof ng.Call || ast instanceof ng.SafeCall) {
    const callee = ast.receiver;
    if ((callee instanceof ng.PropertyRead || callee instanceof ng.SafePropertyRead) && isHostReceiver(callee.receiver)) {
      unresolved.push(`call:${callee.name}`);
    } else {
      collectReads(callee, reads, unresolved);
    }
    for (const a of ast.args) collectReads(a, reads, unresolved);
    return;
  }
  if (ast instanceof ng.BindingPipe) {
    unresolved.push(`pipe:${ast.name}`);
    collectReads(ast.exp, reads, unresolved);
    for (const a of ast.args) collectReads(a, reads, unresolved);
    return;
  }
  for (const [key, value] of Object.entries(ast)) {
    if (key === 'span' || key === 'sourceSpan' || key === 'nameSpan') continue;
    if (Array.isArray(value)) value.forEach((v) => v instanceof ng.AST && collectReads(v, reads, unresolved));
    else if (value instanceof ng.AST) collectReads(value, reads, unresolved);
  }
}

function classOf(decl) {
  let n = decl?.parent;
  while (n && !ts.isClassDeclaration(n)) n = n.parent;
  return n ?? null;
}

/**
 * Translate one Angular project (`tsconfig`) into a SemanticEvidence v1 envelope.
 * `root` is the repository root documents are relative to.
 */
export async function extractEvidence({ tsconfig, root, snapshot }) {
  const projectDir = path.dirname(path.resolve(tsconfig));
  const versions = checkVersions(projectDir);
  const cfg = readConfiguration(path.resolve(tsconfig));
  // The template type checker is the language service's API; the compiler gates it behind this
  // private option (the reason this adapter pins one exact Angular version).
  cfg.options._enableTemplateTypeChecker = true;
  const host = ts.createCompilerHost(cfg.options);
  const program = new NgtscProgram(cfg.rootNames, cfg.options, host);
  await program.loadNgStructureAsync();
  const ttc = program.compiler.getTemplateTypeChecker();
  const tsProgram = program.getTsProgram();
  const checker = tsProgram.getTypeChecker();
  const diagnostics = program.getNgSemanticDiagnostics().length + program.getTsSemanticDiagnostics().length;

  const documents = new Set();
  const definitions = new Map(); // symbol -> fact
  const facts = [];
  const stats = { bindings: 0, external: 0, dom: 0, unsupported: 0, partial_build_diagnostics: diagnostics };

  const classSymbol = (cls) => {
    const sf = cls.getSourceFile();
    const doc = toDocPath(sf.fileName, root);
    if (!doc || sf.isDeclarationFile || !cls.name) return null;
    const symbol = `${doc}#${cls.name.text}`;
    if (!definitions.has(symbol)) {
      const s = sf.getLineAndCharacterOfPosition(cls.name.getStart());
      const e = sf.getLineAndCharacterOfPosition(cls.name.getEnd());
      documents.add(doc);
      definitions.set(symbol, {
        kind: 'definition',
        fact_id: `class ${symbol}`,
        symbol,
        name: cls.name.text,
        site: { document: doc, range: { start_line: s.line, start_col: s.character, end_line: e.line, end_col: e.character } },
      });
    }
    return symbol;
  };

  // The declaration a member is read or written through: a property, or an accessor of the
  // wanted kind. `valueDeclaration` is whichever accessor came first, so search them all.
  const memberDecl = (cls, name, want) => {
    const sym = cls.name && checker.getSymbolAtLocation(cls.name);
    if (!sym) return null;
    const prop = checker.getPropertyOfType(checker.getDeclaredTypeOfSymbol(sym), name);
    const decl = (prop?.declarations ?? []).find(
      (d) => ts.isPropertyDeclaration(d) || (want === 'read' ? ts.isGetAccessorDeclaration(d) : ts.isSetAccessorDeclaration(d)),
    );
    return decl ? { prop, decl } : null;
  };

  const memberOfHost = (hostCls, name) => {
    const m = memberDecl(hostCls, name, 'read');
    const owner = m && classOf(m.decl);
    const cls = owner && classSymbol(owner);
    return cls ? { class: cls, member: m.prop.name } : null;
  };

  // A signal input whose transform is in its initializer (`input(v, {transform})`): the compiler's
  // metadata records no transform for signal inputs, so it is read from the declaration.
  const signalTransform = (decl) => {
    const init = ts.isPropertyDeclaration(decl) ? decl.initializer : null;
    if (!init || !ts.isCallExpression(init)) return false;
    return init.arguments.some(
      (arg) =>
        ts.isObjectLiteralExpression(arg) &&
        arg.properties.some((p) => p.name && (ts.isIdentifier(p.name) || ts.isStringLiteral(p.name)) && p.name.text === 'transform'),
    );
  };

  for (const sf of tsProgram.getSourceFiles()) {
    if (sf.isDeclarationFile || !toDocPath(sf.fileName, root)) continue;
    const visitClasses = (node) => {
      if (ts.isClassDeclaration(node) && node.name) {
        const template = ttc.getTemplate(node);
        if (template) visitTemplate(node, template);
      }
      ts.forEachChild(node, visitClasses);
    };
    visitClasses(sf);
  }

  function visitTemplate(hostCls, template) {
    const owners = [];
    class Visitor extends ng.TmplAstRecursiveVisitor {
      visitElement(el) {
        owners.push(el);
        super.visitElement(el);
        owners.pop();
      }
      visitTemplate(t) {
        // A structural directive's microsyntax inputs (`*myIf="x"`) live in `templateAttrs`, which
        // the recursive visitor does not walk.
        for (const a of t.templateAttrs) if (a instanceof ng.TmplAstBoundAttribute) handle(a, t);
        owners.push(t);
        super.visitTemplate(t);
        owners.pop();
      }
      visitBoundAttribute(attr) {
        handle(attr, owners[owners.length - 1]);
        super.visitBoundAttribute(attr);
      }
    }
    function handle(attr, owner) {
      if (attr.type !== ng.BindingType.Property && attr.type !== ng.BindingType.TwoWay) {
        stats.unsupported += 1; // class/style/attribute/animation bindings
        return;
      }
      if (!(owner instanceof ng.TmplAstElement || owner instanceof ng.TmplAstTemplate)) {
        stats.unsupported += 1; // selectorless component/directive nodes
        return;
      }
      // One mechanism: the compiler's own directive matching and input mapping for this node
      // (aliases, inheritance, signal and decorator inputs, transforms).
      const metas = (ttc.getDirectivesOfNode(hostCls, owner) ?? []).filter(
        // A component never applies to an embedded template; Angular rejects it there.
        (meta) => !(owner instanceof ng.TmplAstTemplate && meta.isComponent),
      );
      // Host directives come back with their EFFECTIVE mapping already applied: for
      // `hostDirectives: [{directive: H, inputs: ['raw: renamed']}]` H's meta binds `renamed` → `raw`
      // and an unexposed input is absent (verified against 22.2.2).
      const matched = metas.flatMap((meta) =>
        (meta.inputs.getByBindingPropertyName(attr.name) ?? []).map((input) => ({ meta, input })),
      );
      if (matched.length === 0) {
        stats.dom += 1; // a DOM property, not a directive input
        return;
      }
      const doc = toDocPath(attr.keySpan.start.file.url, root);
      if (!doc) return;
      for (const { meta, input } of matched) {
        const dirCls = meta.ref.node;
        const m = ts.isClassDeclaration(dirCls) ? memberDecl(dirCls, input.classPropertyName, 'write') : null;
        const owner2 = m && classOf(m.decl);
        const consumerClass = owner2 && classSymbol(owner2);
        if (!consumerClass) {
          stats.external += 1; // the input is declared in a library without source
          continue;
        }
        stats.bindings += 1;
        const reads = [];
        const unresolved = [];
        collectReads(attr.value, reads, unresolved);
        const producers = [];
        for (const r of reads) {
          if (ttc.getExpressionTarget(r, hostCls)) {
            unresolved.push(`local:${r.name}`);
            continue;
          }
          const hm = memberOfHost(hostCls, r.name);
          if (hm) producers.push(hm);
          else unresolved.push(`member:${r.name}`);
        }
        const whole = attr.value instanceof ng.ASTWithSource ? attr.value.ast : attr.value;
        const transformed = input.transform !== null || signalTransform(m.decl);
        const single =
          !transformed &&
          producers.length === 1 &&
          unresolved.length === 0 &&
          (whole instanceof ng.PropertyRead || whole instanceof ng.SafePropertyRead) &&
          isHostReceiver(whole.receiver);
        const two = attr.type === ng.BindingType.TwoWay;
        const k = attr.keySpan;
        documents.add(doc);
        const consumer = { class: consumerClass, member: input.classPropertyName };
        facts.push({
          kind: 'input_binding',
          fact_id: `${doc}:${k.start.line}:${k.start.col}:${attr.name}->${consumer.class}.${consumer.member}`,
          site: { document: doc, range: { start_line: k.start.line, start_col: k.start.col, end_line: k.end.line, end_col: k.end.col } },
          construct: two ? 'angular_two_way_binding' : 'angular_input_binding',
          // Only one whole host member, untransformed, preserves the value. Two-way says nothing
          // about it: `[(v)]="xs[i]"` reads xs and i, neither whole.
          semantics: single ? 'value_preserving' : 'may_influence',
          consumer,
          producers,
          ...(unresolved.length ? { unresolved: [...unresolved].sort() } : {}),
        });
      }
    }
    ng.tmplAstVisitAll(new Visitor(), template);
  }

  const all = [...definitions.values(), ...facts].sort((a, b) => (a.fact_id < b.fact_id ? -1 : a.fact_id > b.fact_id ? 1 : 0));
  const envelope = {
    schema_version: 1,
    producer: {
      name: PRODUCER,
      version: `${ADAPTER_VERSION}+angular.${versions.compiler}`,
      class: 'compiler',
      capabilities: ['definitions', 'input_bindings'],
    },
    snapshot: snapshot ?? toDocPath(path.resolve(tsconfig), root) ?? path.basename(tsconfig),
    documents: [...documents].sort().map((p) => ({ path: p, position_encoding: 'utf16' })),
    facts: all,
  };
  return { envelope, stats };
}
