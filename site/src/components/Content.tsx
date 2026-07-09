import { useState } from 'react'

/* ────────────────────────────────────────────────────────────────────────────
   wicked-estate — the substrate every agent queries.

   SUBSTRATE = sub + stratum. The page is one continuous body of stacked strata:
   code graph · memory · knowledge · requirements↔code · annotations — five layers
   of ONE thing, all keyed to the same stable symbol identity, all stamped with
   confidence + provenance. The visual language is a geological cross-section; the
   centerpiece is a live core sample you query with a confidence dial.
   ──────────────────────────────────────────────────────────────────────────── */

function GitHubIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.02-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 012-.27c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  )
}

// ── Strata metadata: the five layers of the one substrate ───────────────────────
type StratumId = 'graph' | 'memory' | 'knowledge' | 'requirements' | 'annotations'

const STRATA: { id: StratumId; no: string; name: string; depth: string; tools: string }[] = [
  { id: 'graph',        no: '01', name: 'Code graph',        depth: '−0.0m',  tools: '10 tools' },
  { id: 'memory',       no: '02', name: 'Memory',            depth: '−4.2m',  tools: '6 tools'  },
  { id: 'knowledge',    no: '03', name: 'Knowledge',         depth: '−7.8m',  tools: '7 tools'  },
  { id: 'requirements', no: '04', name: 'Requirements ↔ code', depth: '−11.5m', tools: 'traceability' },
  { id: 'annotations',  no: '05', name: 'Annotations',       depth: '−14.0m', tools: 'typed notes' },
]
const stratumName = (id: StratumId) => STRATA.find(s => s.id === id)!.name

// ── Section shell ────────────────────────────────────────────────────────────
function Section({
  id, solid = false, children, className = '',
}: { id?: string; solid?: boolean; children: React.ReactNode; className?: string }) {
  return (
    <section
      id={id}
      className={`strata${solid ? ' strata-solid' : ''} snap-start min-h-screen flex flex-col justify-center py-24 px-7 ${className}`}
    >
      {children}
    </section>
  )
}

// ── 1 · HERO ────────────────────────────────────────────────────────────────
function Hero() {
  return (
    <Section className="!pt-28 overflow-hidden">
      <div className="max-w-6xl mx-auto w-full grid lg:grid-cols-[1.05fr_0.95fr] gap-14 items-center">
        {/* Left — the thesis, committed in sentence one */}
        <div>
          <span className="kicker">wicked-estate · v0.13.0 · the foundation</span>
          <h1 className="mt-6 font-display font-black text-ink text-[2.9rem] sm:text-6xl lg:text-[4.2rem] leading-[0.94]" style={{ fontStretch: '112%' }}>
            The substrate<br />every agent<br />
            <span style={{ color: 'var(--accent)' }}>queries.</span>
          </h1>
          <p className="mt-7 text-lg text-muted leading-relaxed max-w-xl font-sans">
            One local-first MCP server — a single body of stacked strata:{' '}
            <span className="text-ink">code graph</span>, <span className="text-ink">memory</span>,{' '}
            <span className="text-ink">knowledge</span>, <span className="text-ink">requirements↔code</span> and{' '}
            <span className="text-ink">typed annotations</span>. Five layers of one thing, all keyed to the same
            stable symbol identity, all stamped with confidence and provenance.
          </p>
          <p className="mt-3 font-mono text-xs text-faint">
            SQLite by default — zero infrastructure. One flag to PostgreSQL for a shared team graph.
          </p>
          <div className="mt-9 flex flex-col sm:flex-row gap-3">
            <a href="#query" className="btn-primary">Query the substrate ↓</a>
            <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-outline">
              <GitHubIcon /> View on GitHub
            </a>
          </div>
        </div>

        {/* Right — a thin live strata slab (the "core sample" you're about to read) */}
        <div className="relative">
          <div className="rock-panel p-0">
            <div className="flex items-center justify-between px-5 py-3 border-b border-hairline-strong">
              <span className="depth">CORE SAMPLE · applyDiscount</span>
              <span className="tag tag-accent">one identity</span>
            </div>
            <div className="relative">
              {/* the mineral seam runs vertically through every stratum */}
              <div className="seam-line absolute top-4 bottom-4" style={{ left: '30%' }} />
              {STRATA.map((s, i) => (
                <div
                  key={s.id}
                  className="flex items-center gap-4 px-5 py-4 border-b border-hairline last:border-b-0"
                  style={{ background: i % 2 ? 'color-mix(in oklab, var(--ink) 3%, transparent)' : 'transparent' }}
                >
                  <span className="depth w-14 shrink-0">{s.depth}</span>
                  <span className="font-mono text-[0.6rem] text-faint w-6 shrink-0">{s.no}</span>
                  <span className="font-display font-black text-ink text-sm sm:text-base flex-1" style={{ fontStretch: '108%' }}>
                    {s.name}
                  </span>
                  <span className="tag hidden sm:inline">{s.tools}</span>
                </div>
              ))}
            </div>
          </div>
          <p className="mt-3 text-center font-mono text-[0.6rem] text-faint tracking-wide">
            One continuous body — not a graph plus extras.
          </p>
        </div>
      </div>
    </Section>
  )
}

// ── 2 · QUERY THE SUBSTRATE (the signature interaction) ─────────────────────────
type Prov = 'Parsed' | 'SCIP' | 'TSG' | 'ImportMap' | 'Tags' | 'episodic' | 'FTS+RRF' | 'annotation'
interface Fact { stratum: StratumId; text: string; detail?: string; conf: number; prov: Prov; advisory?: boolean }
interface Subject { id: string; label: string; kind: string; sub: string; facts: Fact[] }

const SUBJECTS: Subject[] = [
  {
    id: 'applyDiscount', label: 'applyDiscount', kind: 'symbol', sub: 'src/checkout/price.ts',
    facts: [
      { stratum: 'graph', text: '3 transitive dependents', detail: 'checkout · cartTotal · api/price', conf: 1.0, prov: 'SCIP' },
      { stratum: 'graph', text: 'referralFlow → applyDiscount', detail: 'tag-scan guess, cross-file unverified', conf: 0.3, prov: 'Tags' },
      { stratum: 'memory', text: 'Decision: coupons never stack', detail: 'spike 2026-06 · scope=project:acme', conf: 0.74, prov: 'episodic' },
      { stratum: 'knowledge', text: '[[Pricing Rules]] §Discounts', detail: 'hybrid FTS + vector, RRF fused', conf: 0.88, prov: 'FTS+RRF' },
      { stratum: 'requirements', text: 'satisfies REQ-142 · validated ✓', detail: 'requirement↔code, enforced flag', conf: 1.0, prov: 'Parsed' },
      { stratum: 'annotations', text: 'assumption: max one coupon per cart', detail: 'advisory · survives re-index', conf: 0.7, prov: 'annotation', advisory: true },
    ],
  },
  {
    id: 'REQ-142', label: 'REQ-142', kind: 'requirement', sub: 'Discounts never stack',
    facts: [
      { stratum: 'requirements', text: '2 symbols satisfy REQ-142', detail: 'validateCoupon ✓ validated · applyDiscount ⋯ unvalidated', conf: 1.0, prov: 'Parsed' },
      { stratum: 'graph', text: 'blast-radius of implementers: 3 dependents', detail: 'checkout · cartTotal · api/price', conf: 1.0, prov: 'SCIP' },
      { stratum: 'graph', text: 'candidate impl: legacyDiscount()', detail: 'import-map heuristic, not confirmed', conf: 0.6, prov: 'ImportMap' },
      { stratum: 'knowledge', text: '[[Pricing Spec]] §Stacking rules', detail: 'linked article', conf: 0.85, prov: 'FTS+RRF' },
      { stratum: 'memory', text: 'Decision: enforce at price layer, not cart', conf: 0.70, prov: 'episodic' },
      { stratum: 'annotations', text: 'question: does BOGO count as a coupon?', detail: 'advisory · open', conf: 0.5, prov: 'annotation', advisory: true },
    ],
  },
  {
    id: 'Deployment Runbook', label: 'Deployment Runbook', kind: 'article', sub: 'knowledge · wiki',
    facts: [
      { stratum: 'knowledge', text: '[[Deployment Runbook]] §Rollback', detail: 'hybrid FTS + vector, RRF fused', conf: 0.88, prov: 'FTS+RRF' },
      { stratum: 'knowledge', text: 'relates → [[Incident-2049]]', detail: 'confidence-scored backlink', conf: 0.72, prov: 'FTS+RRF' },
      { stratum: 'graph', text: 'linked code: deploy.ts · rollback.ts', detail: 'article↔code edges', conf: 1.0, prov: 'SCIP' },
      { stratum: 'memory', text: 'Decision: rollback must be idempotent', detail: 'scope=project:acme', conf: 0.80, prov: 'episodic' },
      { stratum: 'requirements', text: 'supports REQ-207 · unvalidated ⋯', conf: 0.6, prov: 'Parsed' },
      { stratum: 'annotations', text: 'note: runbook last drilled 2026-05', conf: 0.6, prov: 'annotation', advisory: true },
    ],
  },
]

function confColor(conf: number) {
  if (conf >= 1.0) return 'var(--accent)'
  if (conf >= 0.8) return 'var(--ink)'
  if (conf >= 0.6) return 'var(--muted)'
  return 'var(--faint)'
}

function QuerySubstrate() {
  const [subjectId, setSubjectId] = useState(SUBJECTS[0].id)
  const [threshold, setThreshold] = useState(0.6)
  const subject = SUBJECTS.find(s => s.id === subjectId)!

  const live = subject.facts.filter(f => f.conf >= threshold)
  const liveStrata = new Set(live.map(f => f.stratum))

  return (
    <Section id="query" solid>
      <div className="max-w-6xl mx-auto w-full">
        <div className="mb-9">
          <span className="kicker">Query the substrate</span>
          <h2 className="mt-4 font-display text-3xl sm:text-[2.6rem] font-black text-ink">
            Pick a subject. Drag the confidence dial. Read one core sample.
          </h2>
          <p className="mt-4 text-muted max-w-2xl font-sans">
            The substrate returns a single dossier assembled live across all five strata at once. Every fact carries its{' '}
            <span className="text-ink">provenance</span> and <span className="text-ink">confidence</span>. Drive the dial
            to 1.0 and only parsed / SCIP facts survive; drop it and the heuristic tag-scan edges reappear —{' '}
            <span className="text-ink">clearly labeled, never silently promoted</span>.
          </p>
        </div>

        {/* Subject picker — three core samples */}
        <div className="flex flex-wrap gap-2 mb-6">
          {SUBJECTS.map(s => {
            const on = s.id === subjectId
            return (
              <button
                key={s.id}
                onClick={() => setSubjectId(s.id)}
                className="text-left rounded-xl px-4 py-3 transition-all"
                style={{
                  background: on ? 'color-mix(in oklab, var(--accent) 12%, var(--rock))' : 'var(--rock)',
                  border: `1px solid ${on ? 'color-mix(in oklab, var(--accent) 55%, var(--hairline))' : 'var(--hairline-strong)'}`,
                }}
              >
                <div className="flex items-center gap-2">
                  <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: on ? 'var(--accent)' : 'var(--faint)' }} />
                  <span className="font-mono text-sm font-semibold" style={{ color: on ? 'var(--ink)' : 'var(--muted)' }}>{s.label}</span>
                  <span className="tag">{s.kind}</span>
                </div>
                <div className="depth mt-1.5 pl-3.5">{s.sub}</div>
              </button>
            )
          })}
        </div>

        <div className="grid lg:grid-cols-[280px_1fr] gap-5">
          {/* Left — confidence dial + core column */}
          <div className="rock-panel p-5 flex flex-col gap-6">
            <div>
              <div className="flex items-baseline justify-between mb-3">
                <span className="kicker">Confidence dial</span>
                <span className="font-mono text-2xl font-black tabular-nums" style={{ color: 'var(--accent)' }}>
                  {threshold.toFixed(2)}
                </span>
              </div>
              <input
                type="range" min={0.3} max={1.0} step={0.05} value={threshold}
                onChange={e => setThreshold(parseFloat(e.target.value))}
                className="dial" aria-label="Confidence threshold"
              />
              <div className="flex justify-between mt-2 depth">
                <span>0.30 · heuristics</span>
                <span>1.00 · SCIP only</span>
              </div>
              <p className="mt-3 font-mono text-[0.62rem] text-muted leading-5">
                {live.length} of {subject.facts.length} facts above cutoff · {liveStrata.size} of 5 strata live
              </p>
            </div>

            {/* the drill core: which strata have a live fact */}
            <div className="mt-auto">
              <span className="kicker">Core column</span>
              <div className="relative mt-3 pl-3">
                <div className="seam-line absolute top-1 bottom-1 left-0" />
                {STRATA.map(s => {
                  const on = liveStrata.has(s.id)
                  return (
                    <div key={s.id} className="flex items-center gap-3 py-1.5">
                      <span className="w-2 h-2 rounded-full shrink-0"
                        style={{ background: on ? 'var(--accent)' : 'var(--hairline-strong)',
                          animation: on ? 'live-pulse 2.4s var(--ease) infinite' : 'none' }} />
                      <span className="depth w-14 shrink-0">{s.depth}</span>
                      <span className="font-mono text-[0.66rem]" style={{ color: on ? 'var(--ink)' : 'var(--faint)' }}>
                        {s.name}
                      </span>
                    </div>
                  )
                })}
              </div>
            </div>
          </div>

          {/* Right — the assembled dossier */}
          <div className="rock-panel p-0">
            <div className="flex items-center justify-between px-5 py-3 border-b border-hairline-strong">
              <span className="font-mono text-sm text-ink font-semibold">{subject.label}</span>
              <span className="depth">substrate.query({subject.kind}) → 1 dossier · 5 strata</span>
            </div>
            <div className="divide-y divide-hairline">
              {STRATA.map(s => {
                const facts = subject.facts.filter(f => f.stratum === s.id)
                if (facts.length === 0) return null
                return (
                  <div key={s.id} className="flex gap-4 px-5 py-4">
                    <div className="w-32 shrink-0 pt-0.5">
                      <div className="font-mono text-[0.6rem] text-faint">{s.no}</div>
                      <div className="font-display font-black text-ink text-sm leading-tight" style={{ fontStretch: '106%' }}>{s.name}</div>
                      <div className="depth mt-0.5">{s.depth}</div>
                    </div>
                    <div className="flex-1 flex flex-col gap-2.5 min-w-0">
                      {facts.map((f, i) => {
                        const on = f.conf >= threshold
                        return (
                          <div key={i} className="fact" style={{ opacity: on ? 1 : 0.32, filter: on ? 'none' : 'grayscale(0.6)' }}>
                            <div className="flex items-start gap-2 flex-wrap">
                              <span className="text-sm font-sans" style={{ color: on ? 'var(--ink)' : 'var(--muted)' }}>{f.text}</span>
                              <span className="prov" style={f.conf >= 1.0 ? { color: 'var(--accent)', borderColor: 'color-mix(in oklab, var(--accent) 45%, var(--hairline))' } : undefined}>
                                {f.prov}
                              </span>
                              <span className="prov tabular-nums" style={{ color: confColor(f.conf) }}>{f.conf.toFixed(2)}</span>
                              {f.advisory && <span className="prov">advisory</span>}
                              {!on && <span className="prov" style={{ color: 'var(--faint)' }}>below cutoff — not promoted</span>}
                            </div>
                            {f.detail && <div className="depth mt-1">{f.detail}</div>}
                          </div>
                        )
                      })}
                    </div>
                  </div>
                )
              })}
            </div>
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 3 · THE FIVE STRATA (one labeled cross-section) ─────────────────────────────
function FiveStrata() {
  const copy: Record<StratumId, string> = {
    graph: 'Symbols, callers, blast-radius, scoped context. source = dependent, target = dependency — always.',
    memory: 'Cross-session recall — decisions, episodes, salience. The decision survives the session that made it.',
    knowledge: 'Ingested articles with hybrid FTS + vector recall, fused via RRF. Answers cite a source you can open.',
    requirements: 'Every symbol carries the requirement it satisfies, a description, and a validated flag.',
    annotations: 'Typed key/value notes — assumption, note, question — with confidence and an advisory flag.',
  }
  return (
    <Section id="strata">
      <div className="max-w-6xl mx-auto w-full">
        <div className="mb-9">
          <span className="kicker">The five strata</span>
          <h2 className="mt-4 font-display text-3xl sm:text-[2.6rem] font-black text-ink">
            Five layers. One substrate. One symbol identity.
          </h2>
          <p className="mt-4 text-muted max-w-2xl font-sans">
            Not a graph with bolt-ons. A single continuous body, cut here into its bands — each keyed to the same
            stable <span className="font-mono text-ink text-sm">(scheme, qualified-name)</span> that survives reformatting,
            moves, and re-index.
          </p>
        </div>

        <div className="rock-panel">
          <div className="relative">
            {/* mineral seam through the whole cross-section */}
            <div className="seam-line absolute top-6 bottom-6" style={{ left: '22%' }} />
            {STRATA.map((s, i) => (
              <div
                key={s.id}
                className="flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-6 px-6 py-6 border-b border-hairline last:border-b-0"
                style={{ background: i % 2 ? 'color-mix(in oklab, var(--ink) 3%, transparent)' : 'transparent' }}
              >
                <div className="flex items-center gap-4 sm:w-64 shrink-0">
                  <span className="font-display font-black text-2xl text-faint tabular-nums" style={{ fontStretch: '108%' }}>{s.no}</span>
                  <div>
                    <div className="font-display font-black text-ink text-lg leading-tight" style={{ fontStretch: '108%' }}>{s.name}</div>
                    <div className="depth mt-0.5">{s.depth} · {s.tools}</div>
                  </div>
                </div>
                <p className="text-sm text-muted font-sans flex-1 leading-relaxed">{copy[s.id]}</p>
              </div>
            ))}
          </div>
        </div>
        <p className="mt-5 font-mono text-xs text-faint">
          23 MCP tools across graph · memory · knowledge, plus requirement↔code traceability and typed annotations — one binary.
        </p>
      </div>
    </Section>
  )
}

// ── 4 · PROVENANCE SEAM (the differentiator, stated once) ───────────────────────
function ProvenanceSeam() {
  const tiers = [
    { tier: 'Parsed',    conf: 1.0, who: 'Direct AST facts' },
    { tier: 'SCIP / LSP', conf: 1.0, who: 'Precise indexers · on-demand' },
    { tier: 'TSG',       conf: 0.8, who: 'Stack-graphs name resolution' },
    { tier: 'ImportMap', conf: 0.6, who: 'Import-map heuristics' },
    { tier: 'Tags',      conf: 0.3, who: 'Tree-sitter tag scan only' },
  ]
  return (
    <Section id="provenance" solid>
      <div className="max-w-5xl mx-auto w-full grid lg:grid-cols-[1fr_1.1fr] gap-12 items-center">
        <div>
          <span className="kicker">The provenance seam</span>
          <h2 className="mt-4 font-display text-3xl sm:text-[2.4rem] font-black text-ink">
            Every edge carries where it came from.
          </h2>
          <p className="mt-4 text-muted font-sans leading-relaxed">
            No edge is emitted without <span className="font-mono text-ink text-sm">confidence</span>,{' '}
            <span className="font-mono text-ink text-sm">provenance</span> and{' '}
            <span className="font-mono text-ink text-sm">resolved_by</span>. On a{' '}
            <span className="font-mono text-ink text-sm">(source, target, kind)</span> collision the higher tier wins —
            a 0.3 tag-scan guess is never presented as a 1.0 fact. You just felt this on the dial.
          </p>
        </div>
        <div className="rock-panel p-6">
          <div className="flex flex-col gap-4">
            {tiers.map(t => (
              <div key={t.tier} className="flex items-center gap-4">
                <span className="font-mono text-xs text-ink w-24 shrink-0">{t.tier}</span>
                <div className="flex-1 h-1.5 rounded-full" style={{ background: 'var(--hairline-strong)' }}>
                  <div className="h-full rounded-full" style={{ width: `${t.conf * 100}%`, background: t.conf >= 1.0 ? 'var(--accent)' : 'var(--muted)', opacity: t.conf >= 1.0 ? 1 : 0.6 }} />
                </div>
                <span className="depth w-8 shrink-0 tabular-nums">{t.conf.toFixed(1)}</span>
                <span className="depth hidden sm:block w-44 shrink-0">{t.who}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 5 · ONE BEDROCK, SOLO OR SHARED (real toggle) ───────────────────────────────
function Bedrock() {
  const [shared, setShared] = useState(false)
  const view = shared
    ? {
        db: 'postgres://team/graph',
        rows: [
          ['writers', 'concurrent — the whole team writes'],
          ['traversal', 'server-side WITH RECURSIVE, in-DB'],
          ['re-index to switch', 'none — same schema, same graph'],
        ],
        note: 'shared_writers=true · server_side_traversal=true',
      }
    : {
        db: 'graph.db',
        rows: [
          ['writers', 'single-writer — local file'],
          ['traversal', 'bounded recursive CTE'],
          ['infrastructure', 'none — nothing to run'],
        ],
        note: 'FTS5 · sqlite-vec · WAL · nothing leaves your box',
      }
  return (
    <Section id="storage">
      <div className="max-w-4xl mx-auto w-full">
        <div className="mb-8">
          <span className="kicker">One bedrock, solo or shared</span>
          <h2 className="mt-4 font-display text-3xl sm:text-[2.4rem] font-black text-ink">
            SQLite by default. One flag to a shared team graph.
          </h2>
          <p className="mt-4 text-muted max-w-2xl font-sans">
            The same engine and command API run on either backend through one{' '}
            <span className="font-mono text-ink text-sm">open_store(spec)</span> factory — no caller changes, no re-index.
          </p>
        </div>

        <div className="inline-flex gap-1.5 p-1.5 rounded-xl mb-5" style={{ background: 'var(--rock)', border: '1px solid var(--hairline-strong)' }}>
          <button className="seg" data-on={!shared} onClick={() => setShared(false)}>SQLite · solo</button>
          <button className="seg" data-on={shared} onClick={() => setShared(true)}>PostgreSQL · shared team</button>
        </div>

        <div className="rock-panel p-0">
          <div className="px-5 py-3 border-b border-hairline-strong flex items-center gap-3">
            <span className="w-2 h-2 rounded-full" style={{ background: shared ? 'var(--accent)' : 'var(--muted)' }} />
            <span className="font-mono text-sm text-ink">wicked-estate index . --db {view.db}</span>
          </div>
          <div className="divide-y divide-hairline">
            {view.rows.map(([k, v]) => (
              <div key={k} className="flex gap-4 px-5 py-3.5">
                <span className="font-mono text-xs text-faint w-40 shrink-0">{k}</span>
                <span className="text-sm text-muted font-sans">{v}</span>
              </div>
            ))}
          </div>
          <div className="px-5 py-3 border-t border-hairline-strong">
            <span className="depth" style={{ color: shared ? 'var(--accent)' : 'var(--faint)' }}>{view.note}</span>
          </div>
        </div>
        <p className="mt-5 font-mono text-xs text-faint">
          PostgreSQL is built behind <span className="text-ink">--features postgres</span>. Same factory arm, zero caller changes.
        </p>
      </div>
    </Section>
  )
}

// ── 6 · THE BEDROCK UNDER THE STACK ─────────────────────────────────────────────
function UnderStack() {
  const layers: { label: string; depth: string; items: { name: string; note: string }[]; bedrock?: boolean }[] = [
    {
      label: 'Products', depth: 'surface',
      items: [
        { name: 'garden', note: 'agent toolkit' },
        { name: 'interactive', note: 'HTML builder' },
        { name: 'studio', note: 'HITL desktop' },
        { name: 'testing', note: 'QE pipeline' },
        { name: 'signals', note: 'intent router' },
      ],
    },
    {
      label: 'Foundation peers', depth: '−6m',
      items: [
        { name: 'wicked-core', note: 'single-writer runtime' },
        { name: 'wicked-bus', note: 'SQLite event substrate' },
        { name: 'wicked-brain', note: 'memory adapter' },
        { name: 'wicked-crew', note: 'workflow governor' },
      ],
    },
  ]
  return (
    <Section id="foundation" solid>
      <div className="max-w-5xl mx-auto w-full">
        <div className="mb-9">
          <span className="kicker">The bedrock under the stack</span>
          <h2 className="mt-4 font-display text-3xl sm:text-[2.6rem] font-black text-ink">
            Estate is the layer everything else rests on.
          </h2>
          <p className="mt-4 text-muted max-w-2xl font-sans">
            The products sit on top. The other foundation repos sit below them. Estate is the bedrock at the bottom —
            the substrate every layer queries.
          </p>
        </div>

        <div className="rock-panel p-0">
          {layers.map(l => (
            <div key={l.label} className="px-6 py-5 border-b border-hairline">
              <div className="flex items-center gap-3 mb-3">
                <span className="depth w-16 shrink-0">{l.depth}</span>
                <span className="kicker">{l.label}</span>
              </div>
              <div className="flex flex-wrap gap-2 pl-16">
                {l.items.map(it => (
                  <span key={it.name} className="tag">
                    <span className="text-ink">{it.name}</span> <span className="text-faint">· {it.note}</span>
                  </span>
                ))}
              </div>
            </div>
          ))}

          {/* the bedrock */}
          <div className="relative px-6 py-8" style={{ background: 'color-mix(in oklab, var(--accent) 8%, transparent)' }}>
            <div className="seam-line absolute top-4 bottom-4" style={{ left: '14%' }} />
            <div className="flex flex-col sm:flex-row sm:items-center gap-4">
              <span className="depth w-16 shrink-0">−14m</span>
              <div className="flex-1">
                <div className="flex items-center gap-3 flex-wrap">
                  <span className="font-display font-black text-ink text-2xl" style={{ fontStretch: '110%' }}>wicked-estate</span>
                  <span className="tag tag-accent">bedrock</span>
                </div>
                <p className="mt-1.5 text-sm text-muted font-sans max-w-xl">
                  Code graph + memory + knowledge + requirements + annotations, in one binary. Every edge stamped with
                  confidence and provenance. The center of gravity everything else queries.
                </p>
              </div>
              <div className="flex flex-wrap gap-1.5 sm:flex-col sm:items-end">
                {['Rust', 'crates.io', 'MCP · 23 tools'].map(t => <span key={t} className="tag">{t}</span>)}
              </div>
            </div>
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 7 · GET STARTED (lean) ──────────────────────────────────────────────────────
function GetStarted() {
  return (
    <Section id="get-started">
      <div className="max-w-4xl mx-auto w-full">
        <div className="mb-8">
          <span className="kicker">Get started</span>
          <h2 className="mt-4 font-display text-3xl sm:text-[2.6rem] font-black text-ink">
            Zero to a queried substrate in two minutes.
          </h2>
        </div>

        <div className="grid md:grid-cols-2 gap-5">
          {/* PRIMARY — installer */}
          <div className="rock-panel p-0">
            <div className="px-5 py-3 border-b border-hairline-strong flex items-center justify-between">
              <span className="kicker" style={{ color: 'var(--accent)' }}>Recommended · the whole family</span>
              <span className="depth">npm</span>
            </div>
            <div className="px-5 py-5">
              <div className="font-mono text-sm text-ink">
                <span style={{ color: 'var(--accent)' }}>$ </span>npx wicked-installer
              </div>
              <p className="mt-3 text-xs text-muted font-sans leading-5">
                Interactive: pick <span className="text-ink">wicked-estate</span> (and any siblings), choose your agent
                CLIs, and it wires everything and ships the cross-family <span className="font-mono text-ink">wicked</span> CLI.
              </p>
            </div>
          </div>

          {/* SECONDARY — direct */}
          <div className="rock-panel p-0">
            <div className="px-5 py-3 border-b border-hairline-strong flex items-center justify-between">
              <span className="kicker">Or install just this directly</span>
              <span className="depth">crates.io</span>
            </div>
            <div className="px-5 py-5 font-mono text-xs leading-[1.9]">
              <div className="text-faint"># the CLI + the MCP server</div>
              <div className="text-ink"><span style={{ color: 'var(--accent)' }}>$ </span>cargo install wicked-estate wicked-estate-mcp</div>
              <div className="text-faint mt-2"># index your repo</div>
              <div className="text-ink"><span style={{ color: 'var(--accent)' }}>$ </span>wicked-estate index . --db graph.db</div>
              <div className="text-faint mt-2"># connect your agent (Claude Code shown)</div>
              <div className="text-ink"><span style={{ color: 'var(--accent)' }}>$ </span>claude mcp add wicked-estate -s project \</div>
              <div className="text-ink pl-4">-- wicked-estate-mcp --db "$PWD/graph.db"</div>
            </div>
          </div>
        </div>

        <p className="mt-4 font-mono text-[0.6rem] text-faint tracking-wide">
          Use an absolute DB path — clients launch from an unpredictable working directory. Same 23 tools in Cursor, Codex, and Antigravity.
        </p>

        <div className="mt-9 flex flex-col sm:flex-row gap-3">
          <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-primary">
            <GitHubIcon /> GitHub
          </a>
          <a href="https://github.com/mikeparcewski/wicked-estate/tree/main/docs" target="_blank" rel="noreferrer" className="btn-outline">
            Documentation
          </a>
        </div>
      </div>
    </Section>
  )
}

// ── Content ──────────────────────────────────────────────────────────────────
export default function Content() {
  return (
    <main className="font-sans">
      <Hero />
      <QuerySubstrate />
      <FiveStrata />
      <ProvenanceSeam />
      <Bedrock />
      <UnderStack />
      <GetStarted />
    </main>
  )
}
