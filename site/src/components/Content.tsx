import { useState, useEffect, useRef } from 'react'

/* ────────────────────────────────────────────────────────────────────────────
   wicked-estate — the substrate every agent queries.

   CONCEPT · "read the core." estate is one durable substrate you read like a
   geologist reads a drill core: a single continuous body, banded into strata
   (graph · memory · knowledge · requirements↔code · annotations), every band
   keyed to one stable symbol identity and stamped with confidence + provenance.
   The signature motion is a DRILL that reads the core — sections demo themselves
   before you touch them. A hard break from the sibling sites' graph-network motif.

   Grounded to v0.13.1 (crates.io): 23 MCP tools across 3 domains (10 estate ·
   6 memory · 7 knowledge), 100+ wired languages, 1,000+ tests, every edge carries
   {confidence, provenance, resolved_by}, injected edges (event→consumer,
   command→agent) grep never sees, SQLite by default / Postgres behind one flag.
   ──────────────────────────────────────────────────────────────────────────── */

function GitHubIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.02-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 012-.27c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  )
}

// respects the OS reduced-motion setting for every auto-animation on the page
function useReducedMotion() {
  const [reduced, setReduced] = useState(false)
  useEffect(() => {
    const mq = window.matchMedia('(prefers-reduced-motion: reduce)')
    setReduced(mq.matches)
    const on = () => setReduced(mq.matches)
    mq.addEventListener('change', on)
    return () => mq.removeEventListener('change', on)
  }, [])
  return reduced
}

// ── Strata metadata: the five bands of the one substrate ────────────────────────
type StratumId = 'graph' | 'memory' | 'knowledge' | 'requirements' | 'annotations'

const STRATA: { id: StratumId; no: string; name: string; depth: string; tools: string; copy: string }[] = [
  { id: 'graph',        no: '01', name: 'Code graph',        depth: '−0.0m',  tools: '10 tools',
    copy: 'Symbols, callers, blast-radius, scoped context — plus injected edges (event→consumer, command→agent) grep can never see.' },
  { id: 'memory',       no: '02', name: 'Memory',            depth: '−4.2m',  tools: '6 tools',
    copy: 'Cross-session recall — decisions, episodes, salience. The decision survives the session that made it.' },
  { id: 'knowledge',    no: '03', name: 'Knowledge',         depth: '−7.8m',  tools: '7 tools',
    copy: 'Ingested articles, hybrid FTS + vector recall fused via RRF. Answers cite a source you can open.' },
  { id: 'requirements', no: '04', name: 'Requirements ↔ code', depth: '−11.5m', tools: 'traceable',
    copy: 'Every symbol carries the requirement it satisfies, a description, and a validated flag.' },
  { id: 'annotations',  no: '05', name: 'Annotations',       depth: '−14.0m', tools: 'typed notes',
    copy: 'Typed key/value notes — assumption, note, question — with confidence and an advisory flag. Survives re-index.' },
]

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

// full-container-width, left-aligned header for sections whose title sits ON TOP of content
function TopHead({ kicker, title, children }: { kicker: string; title: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="mb-8 w-full text-left">
      <span className="kicker">{kicker}</span>
      <h2 className="mt-4 font-display text-3xl sm:text-[2.7rem] font-black text-ink w-full leading-[0.98]">{title}</h2>
      {children && <div className="mt-4 text-ink w-full font-sans leading-relaxed max-w-none">{children}</div>}
    </div>
  )
}

// ── 1 · HERO ────────────────────────────────────────────────────────────────
function Hero() {
  const reduced = useReducedMotion()
  return (
    <Section className="!pt-28 overflow-hidden">
      <div className="max-w-6xl mx-auto w-full grid lg:grid-cols-[1.08fr_0.92fr] gap-14 items-center">
        {/* Left — the thesis, committed in sentence one */}
        <div className="text-left">
          <span className="kicker">wicked-estate · v0.13.1 · crates.io · building blocks</span>
          <h1 className="mt-6 font-display font-black text-ink text-[3rem] sm:text-6xl lg:text-[4.4rem] leading-[0.92]" style={{ fontStretch: '112%' }}>
            The substrate<br />every agent<br />
            <span style={{ color: 'var(--accent)' }}>queries.</span>
          </h1>
          <p className="mt-7 text-lg text-muted leading-relaxed max-w-xl font-sans">
            One local-first MCP server — a single body of stacked strata:{' '}
            <span className="text-ink">code graph</span>, <span className="text-ink">memory</span>,{' '}
            <span className="text-ink">knowledge</span>, <span className="text-ink">requirements↔code</span> and{' '}
            <span className="text-ink">typed annotations</span>. One symbol identity through all five. Every fact
            stamped with confidence and provenance — a heuristic is never handed to an agent as a fact.
          </p>
          <p className="mt-4 font-mono text-xs text-faint leading-5">
            23 tools · 3 domains · 100+ wired languages · SQLite by default, one flag to a shared Postgres graph.
          </p>
          <div className="mt-9 flex flex-col sm:flex-row gap-3">
            <a href="#query" className="btn-primary">Read the core ↓</a>
            <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-outline">
              <GitHubIcon /> View on GitHub
            </a>
          </div>
        </div>

        {/* Right — a live drill core, always scanning (the concept in one glance) */}
        <div className="relative">
          <div className="rock-panel p-0">
            <div className="flex items-center justify-between px-5 py-3 border-b border-hairline-strong">
              <span className="depth">CORE LOG · applyDiscount</span>
              <span className="tag tag-accent">one identity</span>
            </div>
            <div className="relative overflow-hidden">
              {/* the mineral seam runs vertically through every stratum */}
              <div className="seam-line absolute top-3 bottom-3" style={{ left: '30%' }} />
              {/* the drill head sweeps the core forever */}
              {!reduced && (
                <div className="drill-head" style={{ animation: 'drill-sweep 5.5s var(--ease) infinite alternate' }} />
              )}
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

// ── 2 · QUERY THE SUBSTRATE — auto-demos, pauses + becomes yours on click ───────
type Prov = 'Parsed' | 'SCIP' | 'TSG' | 'ImportMap' | 'Tags' | 'Injected' | 'episodic' | 'FTS+RRF' | 'annotation'
interface Fact { stratum: StratumId; text: string; detail?: string; conf: number; prov: Prov; advisory?: boolean }
interface Subject { id: string; label: string; kind: string; facts: Fact[] }

const SUBJECTS: Subject[] = [
  {
    id: 'applyDiscount', label: 'applyDiscount', kind: 'symbol',
    facts: [
      { stratum: 'graph', text: '3 transitive dependents', detail: 'checkout · cartTotal · api/price', conf: 1.0, prov: 'SCIP' },
      { stratum: 'graph', text: 'injected: emits wicked.order.placed → 2 consumers', detail: 'event→consumer edge · grep never sees this', conf: 1.0, prov: 'Injected' },
      { stratum: 'graph', text: 'referralFlow → applyDiscount', detail: 'tag-scan guess · cross-file unverified', conf: 0.3, prov: 'Tags' },
      { stratum: 'memory', text: 'Decision: coupons never stack', detail: 'spike 2026-06 · scope=project:acme', conf: 0.74, prov: 'episodic' },
      { stratum: 'knowledge', text: '[[Pricing Rules]] §Discounts', detail: 'hybrid FTS + vector, RRF fused', conf: 0.88, prov: 'FTS+RRF' },
      { stratum: 'requirements', text: 'satisfies REQ-142 · validated ✓', detail: 'requirement↔code · enforced', conf: 1.0, prov: 'Parsed' },
      { stratum: 'annotations', text: 'assumption: max one coupon per cart', detail: 'advisory · survives re-index', conf: 0.7, prov: 'annotation', advisory: true },
    ],
  },
  {
    id: 'REQ-142', label: 'REQ-142', kind: 'requirement',
    facts: [
      { stratum: 'requirements', text: '2 symbols satisfy REQ-142', detail: 'validateCoupon ✓ · applyDiscount ⋯ unvalidated', conf: 1.0, prov: 'Parsed' },
      { stratum: 'graph', text: 'blast-radius of implementers: 3 dependents', detail: 'checkout · cartTotal · api/price', conf: 1.0, prov: 'SCIP' },
      { stratum: 'graph', text: 'candidate impl: legacyDiscount()', detail: 'import-map heuristic · not confirmed', conf: 0.6, prov: 'ImportMap' },
      { stratum: 'knowledge', text: '[[Pricing Spec]] §Stacking rules', detail: 'linked article', conf: 0.85, prov: 'FTS+RRF' },
      { stratum: 'memory', text: 'Decision: enforce at price layer, not cart', conf: 0.70, prov: 'episodic' },
      { stratum: 'annotations', text: 'question: does BOGO count as a coupon?', detail: 'advisory · open', conf: 0.5, prov: 'annotation', advisory: true },
    ],
  },
  {
    id: 'Deployment Runbook', label: 'Deployment Runbook', kind: 'article',
    facts: [
      { stratum: 'knowledge', text: '[[Deployment Runbook]] §Rollback', detail: 'hybrid FTS + vector, RRF fused', conf: 0.88, prov: 'FTS+RRF' },
      { stratum: 'knowledge', text: 'relates → [[Incident-2049]]', detail: 'confidence-scored backlink', conf: 0.72, prov: 'FTS+RRF' },
      { stratum: 'graph', text: 'injected: command:deploy → deploy-agent', detail: 'command→agent edge · grep never sees this', conf: 1.0, prov: 'Injected' },
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
  const reduced = useReducedMotion()
  const [subjectIdx, setSubjectIdx] = useState(0)
  const [threshold, setThreshold] = useState(0.6)
  const [driving, setDriving] = useState(false)
  // sweep direction for the auto-demo dial
  const dir = useRef(1)

  const subject = SUBJECTS[subjectIdx]
  const live = subject.facts.filter(f => f.conf >= threshold)
  const liveStrata = new Set(live.map(f => f.stratum))

  // auto-demo: sweep the dial 0.3↔1.0; at each turn, advance the subject.
  useEffect(() => {
    if (driving || reduced) return
    const t = setInterval(() => {
      setThreshold(prev => {
        let next = +(prev + dir.current * 0.05).toFixed(2)
        if (next >= 1.0) { next = 1.0; dir.current = -1; setSubjectIdx(i => (i + 1) % SUBJECTS.length) }
        else if (next <= 0.3) { next = 0.3; dir.current = 1 }
        return next
      })
    }, 260)
    return () => clearInterval(t)
  }, [driving, reduced])

  const takeControl = () => setDriving(true)

  return (
    <Section id="query" solid className="!py-16">
      <div className="max-w-6xl mx-auto w-full">
        <div className="mb-6 w-full text-left flex flex-wrap items-end justify-between gap-4">
          <div>
            <span className="kicker">Query the substrate</span>
            <h2 className="mt-4 font-display text-3xl sm:text-[2.6rem] font-black text-ink w-full leading-[0.98]">
              One question. One dossier. Assembled live across all five strata.
            </h2>
            <p className="mt-4 text-ink w-full font-sans leading-relaxed max-w-3xl">
              Watch it read itself — the confidence dial sweeps and the subject changes on its own. Drive the dial to{' '}
              <span className="font-semibold">1.0</span> and only parsed / SCIP facts survive; drop it and the heuristic
              tag-scan edges reappear — <span className="font-semibold">labeled, never silently promoted</span>.
            </p>
          </div>
          <button
            className="demo-pill"
            data-live={String(!driving)}
            onClick={() => setDriving(d => !d)}
            aria-label={driving ? 'Resume auto demo' : 'Pause and drive it yourself'}
          >
            <span className="dot" />
            {driving ? 'You’re driving · resume demo' : 'Auto-demo · click to drive'}
          </button>
        </div>

        {/* Subject picker — three core samples */}
        <div className="flex flex-wrap gap-2 mb-5">
          {SUBJECTS.map((s, i) => {
            const on = i === subjectIdx
            return (
              <button
                key={s.id}
                onClick={() => { takeControl(); setSubjectIdx(i) }}
                className="text-left rounded-xl px-4 py-2.5 transition-all"
                style={{
                  background: on ? 'color-mix(in oklab, var(--accent) 12%, var(--rock))' : 'var(--rock)',
                  border: `1px solid ${on ? 'color-mix(in oklab, var(--accent) 55%, var(--hairline))' : 'var(--hairline-strong)'}`,
                }}
              >
                <div className="flex items-center gap-2">
                  <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ background: on ? 'var(--accent)' : 'var(--faint)' }} />
                  <span className="font-mono text-sm font-semibold text-ink">{s.label}</span>
                  <span className="tag">{s.kind}</span>
                </div>
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
                onChange={e => { takeControl(); setThreshold(parseFloat(e.target.value)) }}
                onMouseDown={takeControl} onTouchStart={takeControl}
                className="dial" aria-label="Confidence threshold"
              />
              <div className="flex justify-between mt-2 depth">
                <span>0.30 · heuristics</span>
                <span>1.00 · SCIP only</span>
              </div>
              <p className="mt-3 font-mono text-[0.62rem] text-ink leading-5">
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
                          animation: on && !reduced ? 'live-pulse 2.4s var(--ease) infinite' : 'none' }} />
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
                  <div key={s.id} className="flex gap-4 px-5 py-3">
                    <div className="w-28 shrink-0 pt-0.5">
                      <div className="font-mono text-[0.6rem] text-faint">{s.no}</div>
                      <div className="font-display font-black text-ink text-sm leading-tight" style={{ fontStretch: '106%' }}>{s.name}</div>
                      <div className="depth mt-0.5">{s.depth}</div>
                    </div>
                    <div className="flex-1 flex flex-col gap-2 min-w-0">
                      {facts.map((f, i) => {
                        const on = f.conf >= threshold
                        const injected = f.prov === 'Injected'
                        return (
                          <div key={i} className="fact min-w-0" style={{ opacity: on ? 1 : 0.42, filter: on ? 'none' : 'grayscale(0.6)' }}>
                            <div className="flex items-center gap-2 min-w-0">
                              <span className="text-sm font-sans truncate min-w-0 flex-1" style={{ color: on ? 'var(--ink)' : 'var(--muted)' }} title={f.text}>{f.text}</span>
                              <span className="prov shrink-0" style={f.conf >= 1.0 || injected ? { color: 'var(--accent)', borderColor: 'color-mix(in oklab, var(--accent) 45%, var(--hairline))' } : undefined}>
                                {f.prov}
                              </span>
                              <span className="prov tabular-nums shrink-0" style={{ color: confColor(f.conf) }}>{f.conf.toFixed(2)}</span>
                              {f.advisory && <span className="prov shrink-0">adv</span>}
                              {!on && <span className="prov shrink-0" style={{ color: 'var(--faint)' }}>below cutoff</span>}
                            </div>
                            {f.detail && <div className="depth mt-0.5 truncate" style={{ color: injected ? 'var(--accent)' : 'var(--muted)' }} title={f.detail}>{f.detail}</div>}
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

// ── 3 · THE FIVE STRATA — a drill reads the cross-section, band by band ─────────
function FiveStrata() {
  const reduced = useReducedMotion()
  const [active, setActive] = useState(0)

  useEffect(() => {
    if (reduced) return
    const t = setInterval(() => setActive(a => (a + 1) % STRATA.length), 1700)
    return () => clearInterval(t)
  }, [reduced])

  return (
    <Section id="strata">
      <div className="max-w-6xl mx-auto w-full">
        <TopHead
          kicker="The five strata"
          title={<>Five bands. One substrate. <span style={{ color: 'var(--accent)' }}>One symbol identity.</span></>}
        >
          <p className="max-w-3xl">
            Not a graph with bolt-ons — a single continuous body, cut into bands. Each keyed to the same stable{' '}
            <span className="font-mono text-sm font-semibold">(scheme, qualified-name)</span> that survives reformatting,
            moves, and re-index. The drill reads one band at a time.
          </p>
        </TopHead>

        <div className="rock-panel p-0">
          <div className="relative">
            <div className="seam-line absolute top-6 bottom-6" style={{ left: '22%' }} />
            {STRATA.map((s, i) => {
              const on = i === active
              return (
                <div
                  key={s.id}
                  onMouseEnter={() => setActive(i)}
                  className="relative flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-6 px-6 py-6 border-b border-hairline last:border-b-0 transition-all duration-300"
                  style={{
                    background: on
                      ? 'color-mix(in oklab, var(--accent) 9%, transparent)'
                      : (i % 2 ? 'color-mix(in oklab, var(--ink) 3%, transparent)' : 'transparent'),
                    boxShadow: on ? 'inset 3px 0 0 var(--accent)' : 'none',
                  }}
                >
                  <div className="flex items-center gap-4 sm:w-64 shrink-0">
                    <span className="font-display font-black text-2xl tabular-nums" style={{ fontStretch: '108%', color: on ? 'var(--accent)' : 'var(--faint)' }}>{s.no}</span>
                    <div>
                      <div className="font-display font-black text-ink text-lg leading-tight" style={{ fontStretch: '108%' }}>{s.name}</div>
                      <div className="depth mt-0.5">{s.depth} · {s.tools}</div>
                    </div>
                  </div>
                  <p className="text-sm font-sans flex-1 leading-relaxed transition-colors" style={{ color: on ? 'var(--ink)' : 'var(--muted)' }}>{s.copy}</p>
                </div>
              )
            })}
          </div>
        </div>
        <p className="mt-5 font-mono text-xs text-faint">
          23 MCP tools across graph · memory · knowledge, plus requirement↔code traceability and typed annotations — one binary.
        </p>
      </div>
    </Section>
  )
}

// ── 4 · PROVENANCE SEAM — cycle the collisions; the winning label pops ──────────
const TIERS: { tier: string; conf: number; who: string }[] = [
  { tier: 'Parsed',     conf: 1.0, who: 'Direct AST facts' },
  { tier: 'SCIP / LSP', conf: 1.0, who: 'Precise indexers · on-demand' },
  { tier: 'TSG',        conf: 0.8, who: 'Stack-graph name resolution' },
  { tier: 'ImportMap',  conf: 0.6, who: 'Import-map heuristics' },
  { tier: 'Tags',       conf: 0.3, who: 'Tree-sitter tag scan only' },
]

// each collision: the same (source,target,kind) edge proposed by several tiers.
// higher tier wins; the losers are superseded. `proposed` = tier indices in play.
const COLLISIONS: { edge: string; kind: string; proposed: number[] }[] = [
  { edge: 'checkout → applyDiscount', kind: 'calls', proposed: [0, 3, 4] },
  { edge: 'price.ts → utils', kind: 'imports', proposed: [1, 3] },
  { edge: 'referralFlow → applyDiscount', kind: 'calls', proposed: [4] },
  { edge: 'service → handler', kind: 'implements', proposed: [1, 2] },
]

function ProvenanceSeam() {
  const reduced = useReducedMotion()
  const [idx, setIdx] = useState(0)
  const [pinned, setPinned] = useState(false)

  useEffect(() => {
    if (reduced || pinned) return
    const t = setInterval(() => setIdx(i => (i + 1) % COLLISIONS.length), 2600)
    return () => clearInterval(t)
  }, [reduced, pinned])

  const c = COLLISIONS[idx]
  const winner = Math.min(...c.proposed) // lowest tier index = highest tier = winner

  return (
    <Section id="provenance" solid>
      <div className="max-w-5xl mx-auto w-full grid lg:grid-cols-[1fr_1.15fr] gap-12 items-center">
        <div className="text-left">
          <span className="kicker">The provenance seam</span>
          <h2 className="mt-4 font-display text-3xl sm:text-[2.4rem] font-black text-ink leading-[0.98]">
            Every edge carries where it came from.
          </h2>
          <p className="mt-4 text-ink font-sans leading-relaxed">
            No edge ships without <span className="font-mono text-ink text-sm">confidence</span>,{' '}
            <span className="font-mono text-ink text-sm">provenance</span> and{' '}
            <span className="font-mono text-ink text-sm">resolved_by</span>. When the same{' '}
            <span className="font-mono text-ink text-sm">(source, target, kind)</span> edge is proposed by several
            tiers, the <span className="font-semibold">highest tier wins</span> — a 0.3 tag-scan guess is never
            presented as a 1.0 fact.
          </p>
          <div className="mt-6 rock-panel p-4">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="depth">collision</span>
              <span className="font-mono text-sm text-ink">{c.edge}</span>
              <span className="tag">{c.kind}</span>
            </div>
            <div className="mt-3 flex items-center gap-2 flex-wrap">
              <span className="depth">resolved_by →</span>
              <span className="tag tag-accent">{TIERS[winner].tier}</span>
              <span className="depth">{c.proposed.length > 1 ? `${c.proposed.length - 1} lower tier(s) superseded` : 'sole proposer'}</span>
            </div>
            <div className="mt-3 flex gap-1.5">
              {COLLISIONS.map((_, i) => (
                <button key={i} onClick={() => { setPinned(true); setIdx(i) }} aria-label={`collision ${i + 1}`}
                  className="h-1.5 rounded-full transition-all"
                  style={{ width: i === idx ? 22 : 8, background: i === idx ? 'var(--accent)' : 'var(--hairline-strong)' }} />
              ))}
              <span className="depth ml-2">{pinned ? 'pinned' : 'cycling'}</span>
            </div>
          </div>
        </div>

        <div className="rock-panel p-6">
          <div className="flex flex-col gap-4">
            {TIERS.map((t, i) => {
              const inPlay = c.proposed.includes(i)
              const isWinner = i === winner
              return (
                <div key={t.tier} className="tier-row flex items-center gap-4" data-in={String(inPlay)}>
                  <span className="font-mono text-xs w-24 shrink-0" style={{ color: isWinner ? 'var(--accent)' : 'var(--ink)' }}>{t.tier}</span>
                  <div className="flex-1 h-1.5 rounded-full" style={{ background: 'var(--hairline-strong)' }}>
                    <div className="h-full rounded-full transition-all duration-500"
                      style={{ width: inPlay ? `${t.conf * 100}%` : '0%', background: isWinner ? 'var(--accent)' : 'var(--muted)', opacity: isWinner ? 1 : 0.55 }} />
                  </div>
                  <span className="depth w-8 shrink-0 tabular-nums">{t.conf.toFixed(1)}</span>
                  <span className="who-label prov hidden sm:block w-48 shrink-0 text-center" data-scope={String(isWinner)}>
                    {t.who}
                  </span>
                </div>
              )
            })}
          </div>
          <p className="mt-5 depth">The right-hand label lights when its tier is the one in scope.</p>
        </div>
      </div>
    </Section>
  )
}

// ── 5 · SOLO OR SHARED — same engine, one flag ──────────────────────────────────
function Storage() {
  const [shared, setShared] = useState(false)
  const view = shared
    ? {
        db: 'postgres://team/graph',
        rows: [
          ['writers', 'concurrent — the whole team writes'],
          ['traversal', 'server-side WITH RECURSIVE, in-DB'],
          ['re-index to switch', 'none — same schema, same graph'],
        ],
        note: '--features postgres · concurrent writers · server-side traversal',
      }
    : {
        db: 'graph.db',
        rows: [
          ['writers', 'single-writer — one local file'],
          ['traversal', 'bounded recursive CTE'],
          ['infrastructure', 'none — nothing to run'],
        ],
        note: 'FTS5 · sqlite-vec · WAL · nothing leaves your box',
      }
  return (
    <Section id="storage" solid>
      <div className="max-w-4xl mx-auto w-full">
        <TopHead
          kicker="One bedrock, solo or shared"
          title={<>SQLite by default. <span style={{ color: 'var(--accent)' }}>One flag</span> to a shared team graph.</>}
        >
          <p className="max-w-3xl">
            The same engine and command API run on either backend through one{' '}
            <span className="font-mono text-sm font-semibold">open_store(spec)</span> factory — no caller changes,
            no re-index. Local-first is a feature, not a ceiling.
          </p>
        </TopHead>

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
                <span className="font-mono text-xs text-muted w-40 shrink-0">{k}</span>
                <span className="text-sm text-ink font-sans">{v}</span>
              </div>
            ))}
          </div>
          <div className="px-5 py-3 border-t border-hairline-strong">
            <span className="depth" style={{ color: shared ? 'var(--accent)' : 'var(--faint)' }}>{view.note}</span>
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 6 · THE BEDROCK UNDER THE STACK — cycle the layers, pointer + what-you-do ────
type LayerId = 'solutions' | 'utilities' | 'building'
const LAYERS: { id: LayerId; label: string; members: { name: string; note: string }[]; doing: string; bedrock?: boolean }[] = [
  {
    id: 'solutions', label: 'Solutions',
    members: [{ name: 'crew', note: 'agentic execution platform' }, { name: 'interactive', note: 'describe-it-build-it docs' }],
    doing: 'Drive governed multi-agent workflows and build interactive docs by describing them. The top of the stack — they read everything below.',
  },
  {
    id: 'utilities', label: 'Utilities',
    members: [{ name: 'garden', note: 'agent toolkit' }, { name: 'testing', note: 'QE team, no self-grading' }],
    doing: 'Prove "done" from evidence and give your agent a QE team that can’t grade its own homework. Both query the substrate for context and edges.',
  },
  {
    id: 'building', label: 'Building Blocks', bedrock: true,
    members: [{ name: 'estate', note: 'the graph · memory · knowledge' }, { name: 'brain', note: 'markdown memory' }, { name: 'bus', note: 'event substrate' }],
    doing: 'Query the graph, recall memory, ride the event bus. estate is the bedrock at the base — the substrate every layer above queries.',
  },
]

function FamilyStack() {
  const reduced = useReducedMotion()
  const [active, setActive] = useState(2) // start on Building Blocks (the bedrock)
  const [pinned, setPinned] = useState(false)

  useEffect(() => {
    if (reduced || pinned) return
    const t = setInterval(() => setActive(a => (a + 1) % LAYERS.length), 2600)
    return () => clearInterval(t)
  }, [reduced, pinned])

  const current = LAYERS[active]

  return (
    <Section id="foundation">
      <div className="max-w-5xl mx-auto w-full">
        <TopHead
          kicker="The bedrock under the stack"
          title={<>Estate is the layer <span style={{ color: 'var(--accent)' }}>everything else rests on.</span></>}
        />

        <div className="grid lg:grid-cols-[0.9fr_1.1fr] gap-8 items-center">
          {/* left — what you DO with the active layer */}
          <div className="text-left">
            <span className="kicker">{current.label}</span>
            <p className="mt-3 text-lg text-ink font-sans leading-relaxed min-h-[7.5rem]">{current.doing}</p>
            <div className="mt-4 flex gap-1.5">
              {LAYERS.map((l, i) => (
                <button key={l.id} onClick={() => { setPinned(true); setActive(i) }} aria-label={l.label}
                  className="h-1.5 rounded-full transition-all"
                  style={{ width: i === active ? 24 : 8, background: i === active ? 'var(--accent)' : 'var(--hairline-strong)' }} />
              ))}
              <span className="depth ml-2">{pinned ? 'pinned' : 'cycling top → bottom'}</span>
            </div>
          </div>

          {/* right — the stack, bottom→top; a left pointer marks the active layer; it shrinks to fit */}
          <div className="rock-panel p-0 overflow-hidden">
            {LAYERS.map((l, i) => {
              const on = i === active
              return (
                <div
                  key={l.id}
                  className="layer-row flex items-stretch"
                  data-active={String(on)}
                  onMouseEnter={() => setActive(i)}
                  style={{ borderBottom: '1px solid var(--hairline)' }}
                >
                  <div className="w-8 flex items-center justify-center shrink-0">
                    {on && <span className="layer-pointer" aria-hidden>{reduced ? '▸' : '►'}</span>}
                  </div>
                  <div className="flex-1 px-4 py-4" style={{ background: l.bedrock ? 'color-mix(in oklab, var(--accent) 7%, transparent)' : 'transparent' }}>
                    <div className="flex items-center gap-3 mb-2 flex-wrap">
                      <span className="kicker" style={{ color: on ? 'var(--accent)' : 'var(--muted)' }}>{l.label}</span>
                      {l.bedrock && <span className="tag tag-accent">bedrock</span>}
                    </div>
                    {/* members show in full only for the active layer — the table shrinks to fit */}
                    {on ? (
                      <div className="flex flex-wrap gap-2">
                        {l.members.map(m => (
                          <span key={m.name} className="tag">
                            <span className="text-ink font-semibold">{m.name}</span> <span className="text-faint">· {m.note}</span>
                          </span>
                        ))}
                      </div>
                    ) : (
                      <div className="font-mono text-[0.66rem] text-faint">{l.members.map(m => m.name).join(' · ')}</div>
                    )}
                  </div>
                </div>
              )
            })}
          </div>
        </div>
        <p className="mt-6 font-mono text-xs text-faint">
          Building Blocks (bottom) → Utilities → Solutions (top). estate · brain · bus carry the load; everything above queries them.
        </p>
      </div>
    </Section>
  )
}

// ── 7 · GET STARTED ─────────────────────────────────────────────────────────────
function GetStarted() {
  return (
    <Section id="get-started" solid>
      <div className="max-w-4xl mx-auto w-full">
        <TopHead
          kicker="Get started"
          title="Zero to a queried substrate in two minutes."
        />

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
                Interactive: pick <span className="text-ink">estate</span> (and any siblings), choose your agent
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
      <Storage />
      <FamilyStack />
      <GetStarted />
    </main>
  )
}
