import { useState, useEffect } from 'react'

// ── Icons ──────────────────────────────────────────────────────────────────────
function GitHubIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M12 0C5.374 0 0 5.373 0 12c0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23A11.509 11.509 0 0 1 12 5.803c1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576C20.566 21.797 24 17.3 24 12c0-6.627-5.373-12-12-12z" />
    </svg>
  )
}

// ── Hero ───────────────────────────────────────────────────────────────────────
function Hero() {
  return (
    <section className="snap-start relative min-h-screen flex flex-col items-center justify-center pt-[58px] overflow-hidden bg-canvas">
      <div className="absolute inset-0 grid-bg opacity-100 pointer-events-none" />

      <div className="relative max-w-5xl mx-auto px-7 text-center">
        <div className="inline-flex items-center gap-2 font-mono text-[0.65rem] tracking-[0.22em] uppercase text-muted border border-hairline-strong rounded-full px-3 py-1.5 mb-8"
          style={{ background: 'color-mix(in oklab, var(--accent) 8%, var(--canvas))' }}>
          <span className="live-dot w-1.5 h-1.5 rounded-full bg-accent inline-block" />
          Local-first · Single binary · Tree-sitter + SQLite · PostgreSQL
        </div>

        <h1 className="font-display text-5xl sm:text-6xl lg:text-[4.2rem] font-black mb-6 text-ink" style={{ fontStretch: '115%' }}>
          The code graph<br />
          <span style={{ color: 'var(--accent)' }}>agents actually trust.</span>
        </h1>

        <p className="text-lg sm:text-xl text-muted max-w-2xl mx-auto mb-10 leading-relaxed font-sans">
          Turn any repo — and its surrounding infrastructure estate — into one queryable graph.
          Every edge carries{' '}
          <span className="text-ink font-mono text-base">confidence</span> and{' '}
          <span className="text-ink font-mono text-base">provenance</span>.
          Heuristics are never presented as facts.
        </p>

        <div className="flex flex-col sm:flex-row gap-3 justify-center mb-16">
          <a href="#get-started" className="btn-primary">Get Started</a>
          <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-outline">
            <GitHubIcon />
            View on GitHub
          </a>
        </div>

        {/* Terminal — always dark */}
        <div className="terminal max-w-2xl mx-auto text-left"
          style={{ boxShadow: '0 40px 80px -30px rgba(0,0,0,0.55)' }}>
          <div className="terminal-bar">
            <div className="terminal-dot bg-red-500/80" />
            <div className="terminal-dot bg-yellow-500/80" />
            <div className="terminal-dot bg-green-500/80" />
            <span className="ml-2 font-mono text-[0.65rem] tracking-widest uppercase text-white/30">blast-radius</span>
          </div>
          <div className="px-5 py-4 space-y-1 text-sm">
            <div>
              <span style={{ color: 'var(--accent)' }}>$</span>
              <span className="text-white/80"> wicked-estate blast-radius </span>
              <span className="text-[#4ade80]">handleRequest</span>
              <span className="text-white/40"> --db graph.db</span>
            </div>
            <div className="pt-2 text-xs leading-[1.8]">
              <div className="text-white/70 mb-1">5 symbol(s) transitively depend on <span className="text-[#4ade80]">'handleRequest'</span>:</div>
              <div className="pl-2 space-y-0.5 text-white/50">
                {[
                  ['authenticate', 'src/middleware.ts'],
                  ['validateToken', 'src/auth.ts'],
                  ['routeRequest', 'src/router.ts'],
                  ['rateLimitCheck', 'src/middleware.ts'],
                  ['main', 'src/server.ts'],
                ].map(([fn, file]) => (
                  <div key={fn}>
                    <span style={{ color: 'var(--accent)' }}>Function</span>{' '}
                    <span className="text-white/80">{fn}</span>
                    <span className="text-white/25 ml-8">{file}</span>
                  </div>
                ))}
              </div>
              <div className="pt-2 text-white/30">coverage: 5 resolved dependent(s); 0 unresolved — SCIP tier active</div>
            </div>
          </div>
        </div>

        <p className="mt-5 font-mono text-xs text-faint">
          Blast-radius = transitive dependents — not text matches.
        </p>
      </div>
    </section>
  )
}

// ── Use Cases ──────────────────────────────────────────────────────────────────
function UseCases() {
  const cases = [
    {
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" strokeWidth={1.5} viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
        </svg>
      ),
      title: 'Blast-radius before every PR',
      body: 'Know exactly which callers, tests, and services break before you merge — not after. Trace every transitive dependent of the function you changed.',
      cmd: 'wicked-estate blast-radius handleRequest',
      out: '5 transitive dependents · 0 unresolved · SCIP tier',
    },
    {
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" strokeWidth={1.5} viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 15.803 7.5 7.5 0 0016.803 15.803z" />
        </svg>
      ),
      title: 'Onboard a new codebase in minutes',
      body: "Rank every symbol by PageRank over real call-edges. Find the entry points, the hot paths, and the dead code — without reading a single file manually.",
      cmd: 'wicked-estate rank --top 20',
      out: '#1 main  ·  #2 handleRequest  ·  #3 authenticate …',
    },
    {
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" strokeWidth={1.5} viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5" />
        </svg>
      ),
      title: 'Retire a deprecated API safely',
      body: 'Search for every caller of the old API across 91 languages in one query. Get a ranked list of files to update — sorted by how often each caller is itself called.',
      cmd: 'wicked-estate callers legacyAuth --db graph.db',
      out: '23 callers found · sorted by PageRank · conf ≥ 0.6',
    },
    {
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" strokeWidth={1.5} viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" d="M8.625 12a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H8.25m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H12m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0h-.375M21 12c0 4.556-4.03 8.25-9 8.25a9.764 9.764 0 01-2.555-.337A5.972 5.972 0 015.41 20.97a5.969 5.969 0 01-.474-.065 4.48 4.48 0 00.978-2.025c.09-.457-.133-.901-.467-1.226C3.93 16.178 3 14.189 3 12c0-4.556 4.03-8.25 9-8.25s9 3.694 9 8.25z" />
        </svg>
      ),
      title: 'Give your agent precise context',
      body: 'Stop sending thousand-line file dumps. The MCP server gives your agent ranked, scoped, provenance-tagged slices — exactly what it needs, nothing it doesn\'t.',
      cmd: 'SearchEntity("login validation")',
      out: '3 results · ranked · confidence tagged · <2K tokens',
    },
    {
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" strokeWidth={1.5} viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
        </svg>
      ),
      title: 'IaC drift detection',
      body: 'Terraform, CloudFormation, Kubernetes — parsed as first-class nodes. Pull live state from AWS or Azure directly, then graph-diff it against your IaC to surface drift before it becomes an incident.',
      cmd: 'wicked-estate drift --db postgres://team/graph',
      out: '3 resources drifted · 1 unmanaged · 2 undeployed',
    },
    {
      icon: (
        <svg className="w-5 h-5" fill="none" stroke="currentColor" strokeWidth={1.5} viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" d="M20.25 6.375c0 2.278-3.694 4.125-8.25 4.125S3.75 8.653 3.75 6.375m16.5 0c0-2.278-3.694-4.125-8.25-4.125S3.75 4.097 3.75 6.375m16.5 0v11.25c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125V6.375m16.5 2.625c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125m16.5 5.625c0 2.278-3.694 4.125-8.25 4.125s-8.25-1.847-8.25-4.125" />
        </svg>
      ),
      title: 'Shared team graph',
      body: 'Switch from SQLite to PostgreSQL with one flag. Multiple CI jobs and agents write concurrently. Everyone queries the same graph — no stale per-developer DBs.',
      cmd: 'wicked-estate index . --db postgres://team/graph',
      out: 'shared_writers=true · server_side_traversal=true',
    },
  ]

  return (
    <section id="use-cases" className="snap-start min-h-screen py-24 px-7 bg-canvas-2">
      <div className="max-w-6xl mx-auto">
        <div className="text-center mb-14">
          <span className="kicker">Use Cases</span>
          <h2 className="font-display text-3xl sm:text-4xl font-black text-ink mb-4">
            What you can do with a real code graph.
          </h2>
          <p className="text-muted max-w-xl mx-auto font-sans">
            Not text search. Not grep. A typed, ranked, cross-language graph with confidence on every edge.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {cases.map(c => (
            <div key={c.title} className="card-hover flex flex-col gap-4">
              <div className="flex items-start gap-3">
                <div className="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                  style={{ background: 'color-mix(in oklab, var(--accent) 12%, var(--canvas))', color: 'var(--accent-ink)' }}>
                  {c.icon}
                </div>
                <div>
                  <h3 className="font-mono text-sm font-semibold text-ink mb-1">{c.title}</h3>
                  <p className="text-xs text-muted leading-5 font-sans">{c.body}</p>
                </div>
              </div>
              <div className="mt-auto rounded-lg overflow-hidden" style={{ background: '#111113', border: '1px solid rgba(255,255,255,0.07)' }}>
                <div className="px-3 py-2 font-mono text-[0.65rem] leading-5">
                  <div><span style={{ color: 'var(--accent)' }}>$</span><span className="text-white/60"> {c.cmd}</span></div>
                  <div className="text-white/30 mt-0.5">{c.out}</div>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

// ── Pipeline ───────────────────────────────────────────────────────────────────
function Pipeline() {
  const Chevron = () => (
    <div className="flex-shrink-0 flex lg:flex-col items-center justify-center lg:self-stretch px-1 py-2 lg:py-0 text-hairline-strong">
      <svg className="hidden lg:block" width="16" height="20" viewBox="0 0 16 20" fill="none">
        <path d="M4 2l8 8-8 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
      </svg>
      <svg className="lg:hidden" width="20" height="14" viewBox="0 0 20 14" fill="none">
        <path d="M2 4l8 6 8-6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
      </svg>
    </div>
  )

  const tiers = [
    { label: 'Tags',      conf: 0.3 },
    { label: 'ImportMap', conf: 0.6 },
    { label: 'TSG',       conf: 0.8 },
    { label: 'SCIP',      conf: 1.0, accent: true },
    { label: 'LSP ᵒᵈ',   conf: 1.0, accent: true },
  ]

  return (
    <section id="pipeline" className="snap-start min-h-screen flex flex-col justify-center py-8 px-7 bg-canvas">
      <div className="max-w-6xl mx-auto w-full">

        <div className="text-center mb-6">
          <span className="kicker">How Code Flows</span>
          <h2 className="font-display text-3xl sm:text-4xl font-black text-ink mb-3">Two phases. One invariant.</h2>
          <p className="text-muted max-w-xl mx-auto font-sans text-sm">
            Parse once into nodes and unresolved references. Resolve separately —
            swap resolution tiers without touching the extractors.
          </p>
        </div>

        {/* 4-stage horizontal pipeline */}
        <div className="flex flex-col lg:flex-row items-stretch gap-0 mb-4">

          {/* Stage 1 — Source */}
          <div className="flex-1 phase-box !p-4">
            <p className="font-mono text-[0.6rem] font-bold tracking-widest uppercase text-faint mb-3">Source</p>
            <div className="space-y-1.5 mb-3">
              {[
                { ext: 'rs', c: '#fb923c' }, { ext: 'ts', c: '#60a5fa' },
                { ext: 'py', c: '#fbbf24' }, { ext: 'go', c: '#34d399' },
                { ext: 'tf', c: '#a78bfa' }, { ext: '…',  c: 'var(--faint)' },
              ].map(f => (
                <div key={f.ext} className="flex items-center gap-2 font-mono text-xs">
                  <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: f.c }} />
                  <span className="text-muted">file.</span>
                  <span style={{ color: f.c }}>{f.ext}</span>
                </div>
              ))}
            </div>
            <div className="flex flex-wrap gap-1 pt-2 border-t border-hairline">
              {['parallel', 'stateless', 'per-file'].map(t => <span key={t} className="lang-tag">{t}</span>)}
            </div>
          </div>

          <Chevron />

          {/* Stage 2 — Extract */}
          <div className="flex-1 phase-box !p-4"
            style={{ borderColor: 'color-mix(in oklab, var(--accent) 30%, var(--hairline-strong))' }}>
            <p className="font-mono text-[0.6rem] font-bold tracking-widest uppercase mb-3"
              style={{ color: 'var(--accent-ink)' }}>Phase 1 — Extract</p>
            <div className="font-mono text-xs text-faint space-y-0.5 mb-3">
              <div>tree-sitter grammar</div>
              <div>+ <span className="text-muted">.scm</span> query file</div>
              <div className="flex flex-wrap gap-1 pt-1.5">
                {['rust', 'ts', 'go', 'cobol', 'hcl', '+86'].map(l => (
                  <span key={l} className="lang-tag">{l}</span>
                ))}
              </div>
            </div>
            <div className="space-y-1.5 pt-2 border-t border-hairline">
              {[
                { label: 'Nodes',         c: '#a78bfa', desc: 'symbols · kinds · spans' },
                { label: 'Local edges',   c: '#60a5fa', desc: 'contains · defines'      },
                { label: 'UnresolvedRefs', c: 'var(--accent)', desc: 'cross-file refs'  },
              ].map(r => (
                <div key={r.label} className="flex items-center gap-1.5 font-mono text-xs">
                  <span className="w-1.5 h-1.5 rounded-sm shrink-0" style={{ background: r.c }} />
                  <span className="text-muted">{r.label}</span>
                  <span className="text-faint text-[0.58rem] hidden sm:inline">{r.desc}</span>
                </div>
              ))}
            </div>
          </div>

          <Chevron />

          {/* Stage 3 — Resolve */}
          <div className="flex-1 phase-box !p-4">
            <p className="font-mono text-[0.6rem] font-bold tracking-widest uppercase text-faint mb-3">Phase 2 — Resolve</p>
            <p className="font-mono text-[0.58rem] text-faint mb-2">Cheap → Precise · higher tier wins</p>
            <div className="space-y-1.5 mb-3">
              {tiers.map(t => (
                <div key={t.label} className="flex items-center gap-2">
                  <span className="font-mono text-[0.62rem] text-muted w-16 shrink-0">{t.label}</span>
                  <div className="flex-1 h-1 rounded-full" style={{ background: 'var(--hairline-strong)' }}>
                    <div className="h-full rounded-full transition-all"
                      style={{ width: `${t.conf * 100}%`, background: t.accent ? 'var(--accent)' : 'var(--muted)', opacity: t.accent ? 1 : 0.55 }} />
                  </div>
                  <span className="font-mono text-[0.58rem] text-faint w-5 shrink-0">{t.conf}</span>
                </div>
              ))}
            </div>
            <p className="font-mono text-[0.55rem] text-faint">ᵒᵈ on-demand only, never bulk</p>
          </div>

          <Chevron />

          {/* Stage 4 — Store */}
          <div className="flex-1 phase-box !p-4">
            <p className="font-mono text-[0.6rem] font-bold tracking-widest uppercase text-faint mb-3">Graph Store</p>
            <div className="flex flex-wrap gap-1 mb-3">
              {['SQLite', 'FTS5', 'sqlite-vec', 'WAL', 'PostgreSQL'].map(t => <span key={t} className="lang-tag">{t}</span>)}
            </div>
            <div className="pt-2 border-t border-hairline">
              <p className="font-mono text-[0.58rem] text-faint tracking-widest uppercase mb-1.5">MCP · 5 tools</p>
              <div className="space-y-0.5">
                {['SearchEntity', 'RetrieveEntity', 'TraverseGraph', 'BlastRadius', 'FetchContent'].map(t => (
                  <div key={t} className="font-mono text-xs" style={{ color: 'var(--accent-ink)' }}>{t}</div>
                ))}
              </div>
            </div>
          </div>

        </div>

        {/* Invariant callout */}
        <div className="flex items-center gap-3 px-4 py-3 rounded-xl border"
          style={{ borderColor: 'color-mix(in oklab, var(--accent) 35%, var(--hairline))', background: 'color-mix(in oklab, var(--accent) 5%, var(--canvas))' }}>
          <div className="w-6 h-6 rounded-md flex items-center justify-center shrink-0"
            style={{ background: 'color-mix(in oklab, var(--accent) 20%, var(--canvas))', color: 'var(--accent-ink)' }}>
            <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
            </svg>
          </div>
          <p className="font-sans text-sm text-muted">
            <span className="font-semibold text-ink">The invariant:</span>{' '}
            resolution is swappable — improve a tier without re-parsing any file.
            A better SCIP index drops in with zero extractor changes.
          </p>
        </div>

      </div>
    </section>
  )
}

// ── Graph Model ────────────────────────────────────────────────────────────────
function GraphModel() {
  return (
    <section id="graph" className="snap-start min-h-screen py-24 px-7 bg-canvas-2">
      <div className="max-w-6xl mx-auto">
        <div className="text-center mb-14">
          <span className="kicker">The Graph</span>
          <h2 className="font-display text-3xl sm:text-4xl font-black text-ink mb-4">
            source = dependent.&nbsp; target = dependency.&nbsp; Always.
          </h2>
          <p className="text-muted max-w-xl mx-auto font-sans">
            The edge direction is a hard invariant enforced by the conformance suite.
            Blast-radius follows every dependency kind, not just calls.
          </p>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-10">
          {/* Edge direction */}
          <div className="card">
            <h3 className="font-mono text-sm font-semibold text-ink mb-5">Edge direction invariant</h3>
            <div className="flex items-center justify-center gap-5 mb-6">
              <div className="text-center">
                <div className="w-18 h-18 rounded-xl border-2 p-4 flex items-center justify-center mb-2"
                  style={{ borderColor: 'color-mix(in oklab, var(--accent) 50%, var(--hairline))', background: 'color-mix(in oklab, var(--accent) 8%, var(--canvas))' }}>
                  <span className="font-mono text-sm font-bold text-ink">A</span>
                </div>
                <span className="font-mono text-[0.6rem] text-faint">source<br />(dependent)</span>
              </div>
              <div className="flex flex-col items-center gap-1">
                <div className="flex items-center gap-1">
                  <div className="w-10 h-px" style={{ background: 'var(--accent)' }} />
                  <svg width="9" height="9" viewBox="0 0 9 9" fill="none" className="text-accent">
                    <path d="M0 4.5L7 4.5M4 1L7.5 4.5L4 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </div>
                <span className="font-mono text-[0.55rem] text-faint">Calls / Imports / Extends</span>
              </div>
              <div className="text-center">
                <div className="w-18 h-18 rounded-xl border-2 p-4 flex items-center justify-center mb-2"
                  style={{ borderColor: 'var(--hairline-strong)', background: 'var(--surface)' }}>
                  <span className="font-mono text-sm font-bold text-ink">B</span>
                </div>
                <span className="font-mono text-[0.6rem] text-faint">target<br />(dependency)</span>
              </div>
            </div>
            <div className="space-y-2 font-mono text-xs">
              <div className="flex items-center gap-2 p-2.5 rounded"
                style={{ background: 'color-mix(in oklab, var(--ink) 5%, var(--canvas))' }}>
                <span className="text-faint">A calls B →</span>
                <span className="text-muted">Edge &#123; source: A, target: B, kind: <span style={{ color: 'var(--accent-ink)' }}>Calls</span> &#125;</span>
              </div>
              <div className="p-3 rounded space-y-1" style={{ background: 'color-mix(in oklab, var(--ink) 4%, var(--canvas))' }}>
                <div className="text-muted"><span className="text-ink font-semibold">Dependencies of X</span> = edges where source == X</div>
                <div className="text-muted"><span className="text-ink font-semibold">Dependents of X</span> = edges where target == X</div>
                <div className="border-t border-hairline pt-2 mt-1" style={{ color: 'var(--accent-ink)', fontWeight: '700' }}>
                  Blast radius = transitive dependents
                </div>
              </div>
            </div>
          </div>

          {/* Confidence */}
          <div className="card">
            <h3 className="font-mono text-sm font-semibold text-ink mb-5">Every edge has a source of truth</h3>
            <div className="space-y-3">
              {[
                { tier: 'Parsed', conf: 1.0, who: 'Direct AST facts — contains, defines' },
                { tier: 'SCIP / LSP', conf: 1.0, who: 'Precise indexers, on-demand LSP' },
                { tier: 'TSG', conf: 0.8, who: 'Stack-graphs name resolution' },
                { tier: 'ImportMap', conf: 0.6, who: 'Import-map heuristics' },
                { tier: 'Heuristic', conf: 0.5, who: 'Synthesizers' },
                { tier: 'Tags', conf: 0.3, who: 'Tree-sitter tag scan only' },
              ].map(t => (
                <div key={t.tier} className="flex items-center gap-3">
                  <span className="font-mono text-xs text-muted w-24 shrink-0">{t.tier}</span>
                  <div className="conf-track">
                    <div className="h-full rounded-full transition-all"
                      style={{ width: `${t.conf * 100}%`, background: t.conf === 1.0 ? 'var(--accent)' : 'var(--hairline-strong)' }} />
                  </div>
                  <span className="font-mono text-xs text-faint w-6 shrink-0">{t.conf}</span>
                </div>
              ))}
            </div>
            <p className="mt-4 text-xs text-muted font-sans leading-5">
              On a <span className="font-mono text-ink">(source, target, kind)</span> collision,
              the higher-confidence edge wins. Low-confidence edges are labeled — never silently promoted.
            </p>
          </div>
        </div>

        {/* Stable identity callout */}
        <div className="card border-hairline-strong">
          <div className="flex items-start gap-4">
            <div className="w-9 h-9 rounded-lg flex items-center justify-center shrink-0 mt-0.5"
              style={{ background: 'color-mix(in oklab, var(--accent) 15%, var(--canvas))', border: '1px solid color-mix(in oklab, var(--accent) 40%, var(--hairline))' }}>
              <svg className="w-4 h-4" style={{ color: 'var(--accent-ink)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M7 20l4-16m2 16l4-16M6 9h14M4 15h14" />
              </svg>
            </div>
            <div>
              <h3 className="font-mono text-sm font-semibold text-ink mb-2">Stable symbol identity — never content-hash or line number</h3>
              <p className="text-sm text-muted leading-6 font-sans">
                Keying a node by a hash of its body or by its line makes every edit look like a delete + re-create:
                edges break, history is lost, blast-radius lies.
                Identity is a stable <span className="font-mono text-ink">(scheme, qualified-name)</span> that
                survives reformatting and line shifts.
              </p>
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}

// ── Agent Contract ─────────────────────────────────────────────────────────────
function AgentContract() {
  const rules = [
    { id: 'R1', title: 'Never return an error early in a session', body: 'A single isError: true early on causes session-wide abandonment. Return a successful empty result with a diagnostic instead.', icon: '↩' },
    { id: 'R2', title: 'Unindexed graph → expose zero tools', body: "If the graph isn't built, the MCP server advertises no tools rather than tools that fail. A tool that exists must work.", icon: '○' },
    { id: 'R3', title: 'Partial coverage is WORSE than none', body: 'A graph covering some files but omitting others misleads the agent. Coverage gaps are always surfaced in diagnostics.', icon: '▲' },
    { id: 'R4', title: 'Cap tool output', body: 'Beyond a budget the agent ignores the output. Rank and budget the answer — a tight, ranked response over a complete dump.', icon: '✂' },
    { id: 'R5', title: 'Always report staleness', body: 'Embed commits_behind in every response. A silently-stale graph is a correctness hazard — not just a quality issue.', icon: '◷' },
    { id: 'R6', title: 'Loud fallback marker', body: 'When the graph can\'t answer and the agent reads files, emit GRAPH-FALLBACK: before any content derived from the file.', icon: '!' },
    { id: 'R7', title: 'Confidence is visible', body: 'Heuristic edges are labeled so the agent weights them appropriately. A 0.5-confidence edge is never presented as a 1.0 fact.', icon: '◉' },
    { id: 'R8', title: 'Results are always ranked', body: 'Every query returns symbols ordered by PageRank × confidence. When the token budget forces a cut, the most important symbols survive — not the most recently indexed.', icon: '↑' },
    { id: 'R9', title: 'Symbol IDs are stable across re-indexes', body: 'An agent can bookmark a symbol ID mid-session and retrieve it after a re-index. Identity is a stable qualified name — it never changes unless the symbol is deleted.', icon: '⊞' },
  ]

  return (
    <section id="agents" className="snap-start min-h-screen py-24 px-7 bg-canvas">
      <div className="max-w-6xl mx-auto">
        <div className="text-center mb-14">
          <span className="kicker">Agent Contract</span>
          <h2 className="font-display text-3xl sm:text-4xl font-black text-ink mb-4">
            Built for agents that can't afford to fail.
          </h2>
          <p className="text-muted max-w-xl mx-auto font-sans">
            Empirical constraints from A/B-validated real agent sessions — not opinions.
            Every retrieval tool honors all nine rules.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3 mb-8">
          {rules.map(r => (
            <div key={r.id} className="rule-card">
              <div className="font-mono text-xl shrink-0 text-ink">{r.icon}</div>
              <div>
                <div className="flex items-baseline gap-2 mb-1.5">
                  <span className="font-mono text-[0.6rem] font-bold text-faint tracking-wide">{r.id}</span>
                  <span className="font-mono text-xs font-semibold text-ink">{r.title}</span>
                </div>
                <p className="text-xs text-muted leading-5 font-sans">{r.body}</p>
              </div>
            </div>
          ))}
        </div>

        {/* Graph-first callout */}
        <div className="card" style={{ borderColor: 'color-mix(in oklab, var(--accent) 35%, var(--hairline))' }}>
          <div className="flex items-start gap-4">
            <div className="w-9 h-9 rounded-lg flex items-center justify-center shrink-0 mt-0.5"
              style={{ background: 'color-mix(in oklab, var(--accent) 15%, var(--canvas))', border: '1px solid color-mix(in oklab, var(--accent) 40%, var(--hairline))' }}>
              <svg className="w-4 h-4" style={{ color: 'var(--accent-ink)' }} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M13 10V3L4 14h7v7l9-11h-7z" />
              </svg>
            </div>
            <div>
              <h3 className="font-mono text-sm font-semibold text-ink mb-2">Graph-first retrieval discipline</h3>
              <p className="text-sm text-muted leading-6 font-sans mb-2">
                Every agent that reads code intelligence state MUST query the graph first.
                Direct file reads are fallback only — and must be announced with a loud{' '}
                <span className="font-mono" style={{ color: 'var(--accent-ink)' }}>GRAPH-FALLBACK:</span> prefix.
              </p>
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}

// ── Languages ──────────────────────────────────────────────────────────────────
function Languages() {
  const langs = [
    'Rust', 'Python', 'TypeScript', 'JavaScript', 'Go', 'Java', 'C', 'C++', 'C#',
    'Ruby', 'COBOL', 'JCL', 'RACF', 'IMS/DBD', 'PL/I', 'RPG', 'Natural', 'Assembler',
    'SQL', 'HCL', 'Terraform', 'CloudFormation', 'Kubernetes', 'Ansible', 'Helm',
    'Bash', 'YAML', 'JSON', 'TOML', 'Swift', 'Kotlin', 'Scala', 'Elixir', 'Erlang',
    'Haskell', 'OCaml', 'Zig', 'Lua', 'PHP', 'R', 'Dart', 'Vue', 'Svelte', '+more',
  ]

  // [left%, top%] across a 600px-tall container — staggered rows, organic feel
  const positions: [number, number][] = [
    [7, 4],  [23, 7],  [42, 3],  [60, 6],  [76, 4],  [91, 8],
    [3, 16], [19, 20], [36, 15], [53, 19], [70, 17], [87, 21],
    [11, 29],[27, 32], [45, 28], [62, 31], [79, 27], [94, 33],
    [5, 42], [21, 45], [38, 41], [56, 44], [73, 40], [89, 46],
    [13, 55],[30, 52], [48, 57], [65, 53], [82, 56],
    [4, 67], [20, 70], [37, 65], [54, 69], [71, 66], [88, 71],
    [9, 80], [26, 77], [43, 82], [60, 79], [77, 83],
    [15, 91],[32, 93], [50, 90], [67, 94],
  ]

  // Float durations vary per node (3.5–5.8s) so they're all out of phase
  const floatDurs = [4.2,3.8,5.1,4.6,3.5,5.5,4.0,4.9,3.7,5.2,4.4,3.9,5.7,4.1,3.6,
                     5.0,4.7,3.8,4.3,5.4,3.9,4.8,3.6,5.1,4.5,3.7,5.3,4.2,3.8,4.9,
                     5.6,3.5,4.4,3.9,5.0,4.7,3.6,5.2,4.1,3.8,4.6,5.3,3.7,4.0]

  return (
    <section className="snap-start min-h-screen flex flex-col justify-center py-12 px-7 bg-canvas-2">
      <div className="max-w-6xl mx-auto w-full">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-12 items-center">
          {/* Left: copy + terminal */}
          <div>
            <span className="kicker">Languages</span>
            <h2 className="font-display text-3xl sm:text-4xl font-black text-ink mb-6">
              91 wired languages.<br />
              <span className="text-muted">Zero core changes to add more.</span>
            </h2>
            <p className="text-muted leading-7 mb-6 font-sans">
              Extraction logic lives in <span className="font-mono text-ink">.scm</span> query files
              and a manifest — never in compiled <span className="font-mono text-ink">match language &#123;…&#125;</span> arms.
              A new language is a manifest row and a query file. The capability matrix generates itself.
            </p>

            <div className="terminal text-xs" style={{ boxShadow: '0 20px 48px -24px rgba(0,0,0,0.45)' }}>
              <div className="terminal-bar">
                <div className="terminal-dot bg-red-500/80" />
                <div className="terminal-dot bg-yellow-500/80" />
                <div className="terminal-dot bg-green-500/80" />
                <span className="ml-2 font-mono text-[0.6rem] tracking-widest uppercase text-white/30">languages.toml · adding a language</span>
              </div>
              <div className="px-4 py-4 space-y-0.5 leading-[1.8] font-mono text-xs">
                <div className="text-white/25"># existing</div>
                <div><span style={{ color: 'var(--accent)' }}>[[language]]</span></div>
                <div><span className="text-[#60a5fa]">name</span><span className="text-white/40"> = </span><span className="text-[#4ade80]">"rust"</span></div>
                <div><span className="text-[#60a5fa]">extensions</span><span className="text-white/40"> = </span><span className="text-[#4ade80]">["rs"]</span></div>
                <div className="mt-2 text-white/25"># new language — zero core change</div>
                <div><span style={{ color: 'var(--accent)' }}>[[language]]</span></div>
                <div><span className="text-[#60a5fa]">name</span><span className="text-white/40"> = </span><span className="text-[#4ade80]">"zig"</span></div>
                <div><span className="text-[#60a5fa]">extensions</span><span className="text-white/40"> = </span><span className="text-[#4ade80]">["zig"]</span></div>
                <div className="mt-2 text-white/25"># + add src/queries/zig.scm → done.</div>
              </div>
            </div>

            <p className="mt-4 font-mono text-xs text-faint leading-5">
              A fix in one extractor is a hypothesis about all others —<br />patch the shared seam, not N copies.
            </p>
          </div>

          {/* Right: floating node cloud */}
          <div
            className="relative hidden lg:block"
            style={{
              height: '600px',
              maskImage: 'linear-gradient(to bottom, transparent 0%, black 10%, black 88%, transparent 100%)',
            }}
          >
            {langs.map((l, i) => {
              const [left, top] = positions[i] ?? [50, 50]
              const popDelay = i * 0.07
              const floatDur = floatDurs[i] ?? 4.5
              const isMore = l === '+more'
              return (
                <div
                  key={l}
                  style={{
                    position: 'absolute',
                    left: `${left}%`,
                    top: `${top}%`,
                    transform: 'translate(-50%, -50%)',
                  }}
                >
                  <span
                    className="lang-tag-lg"
                    style={{
                      display: 'block',
                      animationName: 'node-pop, node-float',
                      animationDuration: `0.5s, ${floatDur}s`,
                      animationDelay: `${popDelay}s, ${popDelay + 0.5}s`,
                      animationTimingFunction: 'cubic-bezier(0.34,1.56,0.64,1), ease-in-out',
                      animationFillMode: 'both, none',
                      animationIterationCount: '1, infinite',
                      ...(isMore ? {
                        color: 'var(--accent-ink)',
                        borderColor: 'color-mix(in oklab, var(--accent) 40%, var(--hairline))',
                        background: 'color-mix(in oklab, var(--accent) 8%, var(--canvas))',
                      } : {}),
                    }}
                  >
                    {l}
                  </span>
                </div>
              )
            })}
          </div>
        </div>
      </div>
    </section>
  )
}

// ── MCP Connect ────────────────────────────────────────────────────────────────
function MCPConnect() {
  const [tab, setTab] = useState<'claude' | 'cursor' | 'codex'>('claude')

  const configs: Record<string, string> = {
    claude: `# Claude Code (project scope)
$ claude mcp add wicked-estate -s project \\
    -- wicked-estate-mcp \\
    --db "$PWD/.wicked-estate/graph.db"

# Or install the bundled plugin:
/plugin marketplace add mikeparcewski/wicked-estate`,
    cursor: `// ~/.cursor/mcp.json  (or .cursor/mcp.json per-project)
{
  "mcpServers": {
    "wicked-estate": {
      "command": "wicked-estate-mcp",
      "args": ["--db", "/abs/path/to/.wicked-estate/graph.db"]
    }
  }
}`,
    codex: `# ~/.codex/config.toml
[mcp_servers.wicked-estate]
command = "wicked-estate-mcp"
args = ["--db", "/abs/path/to/.wicked-estate/graph.db"]

# or via CLI:
$ codex mcp add wicked-estate \\
    -- wicked-estate-mcp --db /abs/path/to/graph.db`,
  }

  return (
    <section id="connect" className="snap-start min-h-screen py-24 px-7 bg-canvas">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-14">
          <span className="kicker">Connect an Agent</span>
          <h2 className="font-display text-3xl sm:text-4xl font-black text-ink mb-4">
            One config. Every major client.
          </h2>
          <p className="text-muted max-w-xl mx-auto font-sans">
            <span className="font-mono text-ink">wicked-estate-mcp</span> is a JSON-RPC 2.0 stdio server.
            Register it in Claude Code, Cursor, Antigravity, or Codex.
          </p>
        </div>

        {/* 5 tools */}
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-2 mb-10">
          {[
            { name: 'SearchEntity', desc: 'Find symbols by name or semantic query' },
            { name: 'RetrieveEntity', desc: 'Fetch a full node by stable symbol ID' },
            { name: 'TraverseGraph', desc: 'Multi-hop traversal from a symbol' },
            { name: 'BlastRadius', desc: 'All transitive dependents of a symbol' },
            { name: 'FetchContent', desc: 'Source text stored for a symbol' },
          ].map(t => (
            <div key={t.name} className="card text-center">
              <p className="font-mono text-xs font-bold mb-1.5" style={{ color: 'var(--accent-ink)' }}>{t.name}</p>
              <p className="text-xs text-faint leading-4 font-sans">{t.desc}</p>
            </div>
          ))}
        </div>

        {/* Workflow */}
        <div className="flex flex-wrap items-center gap-2 justify-center mb-10 font-mono text-xs">
          {[
            { step: '1', text: 'index <repo>', color: '' },
            null,
            { step: '2', text: 'launch mcp', color: '' },
            null,
            { step: '3', text: 'SearchEntity(…)', color: 'accent-ink' },
            null,
            { step: '4', text: 'BlastRadius(id)', color: 'accent-ink' },
          ].map((item, i) =>
            item === null ? (
              <svg key={i} className="w-3 h-3 text-faint" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
              </svg>
            ) : (
              <div key={i} className="flex items-center gap-1.5">
                <span className="w-5 h-5 rounded-full border border-hairline-strong flex items-center justify-center text-[0.55rem] font-bold text-faint shrink-0">
                  {item.step}
                </span>
                <span style={{ color: item.color ? 'var(--accent-ink)' : 'var(--muted)' }}>{item.text}</span>
              </div>
            )
          )}
        </div>

        {/* Client tabs */}
        <div className="terminal" style={{ boxShadow: '0 20px 48px -24px rgba(0,0,0,0.45)' }}>
          <div className="terminal-bar gap-3">
            <div className="terminal-dot bg-red-500/80" />
            <div className="terminal-dot bg-yellow-500/80" />
            <div className="terminal-dot bg-green-500/80" />
            <div className="ml-2 flex gap-1">
              {(['claude', 'cursor', 'codex'] as const).map(t => (
                <button
                  key={t}
                  onClick={() => setTab(t)}
                  className="font-mono text-[0.65rem] px-2 py-0.5 rounded transition-colors"
                  style={{
                    background: tab === t ? 'rgba(255,255,255,0.1)' : 'transparent',
                    color: tab === t ? 'rgba(255,255,255,0.85)' : 'rgba(255,255,255,0.35)',
                  }}
                >
                  {t === 'claude' ? 'Claude Code' : t === 'cursor' ? 'Cursor' : 'Codex CLI'}
                </button>
              ))}
            </div>
          </div>
          <pre className="px-5 py-4 text-xs font-mono overflow-x-auto whitespace-pre leading-[1.8]">
            {configs[tab].split('\n').map((line, i) => {
              const isComment = line.trim().startsWith('#') || line.trim().startsWith('//')
              const isKeyword = line.includes('wicked-estate-mcp') || line.includes('--db')
              return (
                <div key={i}>
                  <span style={{
                    color: isComment ? 'rgba(255,255,255,0.25)'
                      : isKeyword ? 'var(--accent)'
                        : 'rgba(255,255,255,0.6)',
                  }}>{line}</span>
                </div>
              )
            })}
          </pre>
        </div>

        <p className="mt-3 font-mono text-[0.6rem] text-faint text-center tracking-wide">
          Use an absolute DB path — clients launch with an unpredictable working directory.
        </p>
      </div>
    </section>
  )
}

// ── Step Player ────────────────────────────────────────────────────────────────
function StepPlayer() {
  const DURATION = 4200
  const steps = [
    {
      label: '01 · install',
      cmd: ['cargo install wicked-estate'],
      out: [
        'Updating crates.io index...',
        'Compiling wicked-estate-core v0.1.7',
        'Compiling wicked-estate-mcp  v0.1.7',
        'Finished  release [optimized]',
        '✓  Installed wicked-estate v0.1.7',
      ],
    },
    {
      label: '02 · index',
      cmd: ['wicked-estate index . --db graph.db'],
      out: [
        'Walking 1,247 source files...',
        'Extractor: 91 languages active',
        'Resolver:  SCIP tier engaged',
        '✓  43,821 symbols · 8,312 edges',
        '   graph.db  2.1 MB · 0 unresolved',
      ],
    },
    {
      label: '03 · query',
      cmd: ['wicked-estate blast-radius handleRequest'],
      out: [
        '5 transitive dependents:',
        '  authenticate    src/middleware.ts',
        '  validateToken   src/auth.ts',
        '  routeRequest    src/router.ts',
        '  rateLimitCheck  src/middleware.ts',
        '  main            src/server.ts',
      ],
    },
    {
      label: '04 · connect',
      cmd: [
        'claude mcp add wicked-estate -s project \\',
        '  -- wicked-estate-mcp --db "$PWD/graph.db"',
      ],
      out: [
        'Registering MCP server...',
        '✓  wicked-estate registered (project)',
        '   SearchEntity  · RetrieveEntity',
        '   TraverseGraph · BlastRadius',
        '   FetchContent',
        '   Your agent has a real code graph.',
      ],
    },
  ]

  const [step, setStep] = useState(0)
  const [visible, setVisible] = useState(true)

  const goTo = (i: number) => {
    setVisible(false)
    setTimeout(() => { setStep(i); setVisible(true) }, 250)
  }

  useEffect(() => {
    const t = setInterval(() => goTo((step + 1) % steps.length), DURATION)
    return () => clearInterval(t)
  }, [step])

  const s = steps[step]

  return (
    <div className="terminal flex flex-col" style={{ boxShadow: '0 20px 48px -24px rgba(0,0,0,0.45)' }}>
      {/* Bar */}
      <div className="terminal-bar justify-between flex-shrink-0">
        <div className="flex items-center gap-1.5">
          <div className="terminal-dot bg-red-500/80" />
          <div className="terminal-dot bg-yellow-500/80" />
          <div className="terminal-dot bg-green-500/80" />
        </div>
        <div className="flex items-center gap-1.5">
          {steps.map((_, i) => (
            <button
              key={i}
              onClick={() => goTo(i)}
              className="w-5 h-5 rounded-full font-mono text-[0.5rem] font-bold transition-all duration-200 flex items-center justify-center"
              style={{
                background: i === step ? 'var(--accent)' : 'rgba(255,255,255,0.12)',
                color: i === step ? '#232324' : 'rgba(255,255,255,0.35)',
              }}
            >
              {i + 1}
            </button>
          ))}
        </div>
      </div>

      {/* Progress bar */}
      <div className="h-px flex-shrink-0" style={{ background: 'rgba(255,255,255,0.06)' }}>
        <div
          key={`prog-${step}`}
          className="h-full"
          style={{ background: 'var(--accent)', animation: `step-progress ${DURATION}ms linear forwards` }}
        />
      </div>

      {/* Content */}
      <div className="flex-1 px-5 py-5 font-mono text-xs leading-[1.75]"
        style={{ opacity: visible ? 1 : 0, transition: 'opacity 0.25s ease' }}>
        <div key={`content-${step}`}>
          <div className="mb-3">
            <span className="font-bold tracking-widest uppercase text-[0.6rem]"
              style={{ color: 'var(--accent)' }}>{s.label}</span>
          </div>
          <div className="mb-4 space-y-0.5">
            {s.cmd.map((line, i) => (
              <div key={i} className="flex gap-1.5">
                {i === 0 && <span style={{ color: 'var(--accent)' }}>$</span>}
                {i > 0 && <span className="opacity-0">$</span>}
                <span className="text-white/75">{line}</span>
              </div>
            ))}
          </div>
          <div className="space-y-0.5 border-t border-white/5 pt-3">
            {s.out.map((line, i) => (
              <div key={i}
                style={{ animationDelay: `${i * 0.1}s`, animation: 'line-fade 0.35s ease both' }}>
                <span style={{ color: line.startsWith('✓') ? '#4ade80' : 'rgba(255,255,255,0.38)' }}>
                  {line}
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

// ── Get Started ────────────────────────────────────────────────────────────────
function GetStarted() {
  const quickStart = `#!/usr/bin/env bash
# install
cargo install wicked-estate

# index your repo (incremental on repeat runs)
wicked-estate index . --db graph.db

# connect to Claude Code
claude mcp add wicked-estate -s project \\
  -- wicked-estate-mcp --db "$PWD/graph.db"

# done — your agent now has a real code graph`

  return (
    <section id="get-started" className="snap-start min-h-screen flex flex-col justify-center py-10 px-7 bg-canvas-2">
      <div className="max-w-5xl mx-auto w-full">
        <div className="text-center mb-8">
          <span className="kicker">Get Started</span>
          <h2 className="font-display text-3xl sm:text-4xl font-black text-ink mb-4">
            One script. Zero to graph.
          </h2>
          <p className="text-muted max-w-md mx-auto font-sans">
            Install, index, connect. Your agent has a real code graph in under two minutes.
          </p>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 items-stretch">
          {/* Left — script terminal */}
          <div className="terminal" style={{ boxShadow: '0 32px 64px -28px rgba(0,0,0,0.5)' }}>
            <div className="terminal-bar">
              <div className="terminal-dot bg-red-500/80" />
              <div className="terminal-dot bg-yellow-500/80" />
              <div className="terminal-dot bg-green-500/80" />
              <span className="ml-2 font-mono text-[0.6rem] tracking-widest uppercase text-white/25">quick-start.sh</span>
            </div>
            <pre className="px-5 py-5 text-sm font-mono overflow-x-auto whitespace-pre leading-[1.85]">
              {quickStart.split('\n').map((line, i) => {
                const isComment = line.trim().startsWith('#')
                const isBlank = line.trim() === ''
                return (
                  <div key={i} className={isBlank ? 'h-3' : ''}>
                    {!isBlank && (
                      <span style={{ color: isComment ? 'rgba(255,255,255,0.28)' : 'rgba(255,255,255,0.72)' }}>
                        {isComment ? line : <><span style={{ color: 'var(--accent)' }}>  </span>{line}</>}
                      </span>
                    )}
                  </div>
                )
              })}
            </pre>
          </div>

          {/* Right — animated step player */}
          <StepPlayer />
        </div>

        <div className="mt-12 flex flex-col sm:flex-row gap-3 justify-center">
          <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-primary">
            <GitHubIcon />
            GitHub
          </a>
          <a href="https://github.com/mikeparcewski/wicked-estate/tree/main/docs" target="_blank" rel="noreferrer" className="btn-outline">
            Documentation
          </a>
        </div>
      </div>
    </section>
  )
}

// ── Content ──────────────────────────────────────────────────────────────────
// Body sections only. The shared wicked-web Topbar + Footer wrap this island
// in src/pages/index.astro; theme is driven by data-theme on <html>.
export default function Content() {
  return (
    <main className="font-sans">
      <Hero />
      <UseCases />
      <Pipeline />
      <GraphModel />
      <AgentContract />
      <Languages />
      <MCPConnect />
      <GetStarted />
    </main>
  )
}
