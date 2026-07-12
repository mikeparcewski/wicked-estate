import { useState, useEffect, useRef } from 'react'

/* ────────────────────────────────────────────────────────────────────────────
   wicked-estate — EQUIP · your live technical environment, queryable.

   ROLE · estate is the Equip layer of the wicked loop: the technical environment
   coding agents read before they act. Not the symbol/knowledge graph itself (that
   memory + code-graph lives in its Equip peer, wicked-brain) — estate is the
   environment those symbols sit in: requirements↔implementation, blast-radius of
   change, infra + policy relationships, and operational history.

   CONCEPT · "read the core." estate is one durable environment you read like a
   geologist reads a drill core: a single continuous body, banded into strata
   (requirements↔impl · blast-radius · infra + policy · operational history ·
   annotations), every band keyed to one stable symbol identity and stamped with
   confidence + provenance. The signature motion is a DRILL that reads the core —
   sections demo themselves before you touch them.

   Grounded to v0.13.1 (crates.io): 100+ wired languages, 1,000+ tests, every edge
   carries {confidence, provenance, resolved_by}, injected edges (event→consumer,
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

// ── Strata metadata: the five bands of the technical environment ────────────────
type StratumId = 'requirements' | 'blast' | 'infra' | 'history' | 'annotations'

const STRATA: { id: StratumId; no: string; name: string; depth: string; tools: string; copy: string }[] = [
  { id: 'requirements', no: '01', name: 'Requirements ↔ implementation', depth: '−0.0m', tools: 'traceable',
    copy: 'Every symbol carries the requirement it satisfies, a description, and a validated flag — the spec pinned to the code that fulfils it.' },
  { id: 'blast',        no: '02', name: 'Blast-radius',      depth: '−4.2m',  tools: 'bounded',
    copy: 'What breaks if you change it — bounded reverse-reachability over every dependency edge kind, plus injected edges (event→consumer, command→agent) grep can never see.' },
  { id: 'infra',        no: '03', name: 'Infra + policy',    depth: '−7.8m',  tools: 'cross-domain',
    copy: 'IaC, mainframe security, data and messaging joined cross-domain — the RACF profile that protects the dataset a JCL step uses, in one query.' },
  { id: 'history',      no: '04', name: 'Operational history', depth: '−11.5m', tools: 'git-aware',
    copy: 'Per-file git provenance, a read-only edge-history log, and drift — iac vs live by resource identity. What the environment was, not only what it is.' },
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
      className={`strata${solid ? ' strata-solid' : ''} px-7 ${className}`}
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
  const [activeRow, setActiveRow] = useState(0)
  const rowsRef = useRef<(HTMLDivElement | null)[]>([])
  const [underline, setUnderline] = useState({ top: 0, left: 0, width: 0 })

  // step the underline block-by-block — dwell on each strata row, then advance
  useEffect(() => {
    if (reduced) return
    const t = setInterval(() => setActiveRow(a => (a + 1) % STRATA.length), 1600)
    return () => clearInterval(t)
  }, [reduced])

  // place the underline at the base of the active block; re-measure on resize
  useEffect(() => {
    const measure = () => {
      const el = rowsRef.current[activeRow]
      if (!el) return
      setUnderline({ top: el.offsetTop + el.offsetHeight - 2, left: el.offsetLeft, width: el.offsetWidth })
    }
    measure()
    window.addEventListener('resize', measure)
    return () => window.removeEventListener('resize', measure)
  }, [activeRow])

  return (
    <Section className="!pt-28 overflow-hidden">
      <div className="max-w-6xl mx-auto w-full grid lg:grid-cols-[1.08fr_0.92fr] gap-14 items-center">
        {/* Left — the thesis, committed in sentence one */}
        <div className="text-left">
          <span className="kicker">wicked-estate · Equip · v0.13.1 · crates.io</span>
          <h1 className="mt-6 font-display font-black text-ink text-[3rem] sm:text-6xl lg:text-[4.4rem] leading-[0.92]" style={{ fontStretch: '112%' }}>
            Your live<br />technical<br />environment,<br />
            <span style={{ color: 'var(--accent)' }}>queryable.</span>
          </h1>
          <p className="mt-7 text-lg text-muted leading-relaxed max-w-xl font-sans">
            One local-first MCP server that maps the environment <span className="italic">around</span> your code —{' '}
            <span className="text-ink">requirements↔implementation</span>, <span className="text-ink">blast-radius of change</span>,{' '}
            <span className="text-ink">infra + policy relationships</span> and <span className="text-ink">operational history</span>.
            Every fact stamped with confidence and provenance — a heuristic is never handed to an agent as a fact.
          </p>
          <p className="mt-4 text-sm text-muted leading-relaxed max-w-xl font-sans">
            The memory and code-graph proper live in <span className="text-ink">wicked-brain</span>, Equip’s other half.
            estate is the technical environment those symbols sit in: what a change touches, what protects it, what it was.
          </p>
          <p className="mt-4 font-mono text-xs text-faint leading-5">
            100+ wired languages · every edge {'{'}confidence, provenance, resolved_by{'}'} · SQLite by default, one flag to a shared Postgres backend.
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
              {/* a single scan line that STEPS down the core, settling under one
                  block at a time — a moving underline that pauses on each block */}
              {!reduced && (
                <div className="drill-underline" style={{ top: underline.top, left: underline.left, width: underline.width }} />
              )}
              {STRATA.map((s, i) => {
                const on = i === activeRow
                return (
                  <div
                    key={s.id}
                    ref={el => { rowsRef.current[i] = el }}
                    onMouseEnter={() => setActiveRow(i)}
                    className="flex items-center gap-4 px-5 py-4 border-b border-hairline last:border-b-0 transition-colors duration-300"
                    style={{ background: on
                      ? 'color-mix(in oklab, var(--accent) 10%, transparent)'
                      : (i % 2 ? 'color-mix(in oklab, var(--ink) 3%, transparent)' : 'transparent') }}
                  >
                    <span className="depth w-14 shrink-0">{s.depth}</span>
                    <span className="font-mono text-[0.6rem] text-faint w-6 shrink-0">{s.no}</span>
                    <span className="font-display font-black text-ink text-sm sm:text-base flex-1" style={{ fontStretch: '108%' }}>
                      {s.name}
                    </span>
                    <span className="tag hidden sm:inline">{s.tools}</span>
                  </div>
                )
              })}
            </div>
          </div>
          <p className="mt-3 text-center font-mono text-[0.6rem] text-faint tracking-wide">
            One continuous body — the whole environment, one identity.
          </p>
        </div>
      </div>
    </Section>
  )
}

// ── 2 · QUERY THE SUBSTRATE — auto-demos, pauses + becomes yours on click ───────
type Prov = 'Parsed' | 'SCIP' | 'TSG' | 'ImportMap' | 'Tags' | 'Injected' | 'git' | 'Collector' | 'annotation'
interface Fact { stratum: StratumId; text: string; detail?: string; conf: number; prov: Prov; advisory?: boolean }
// `dial` is the confidence value the dial SWEEPS TO when this subject is on screen —
// each subject reads out at a different cutoff so the needle visibly travels between tabs.
interface Subject { id: string; label: string; kind: string; dial: number; facts: Fact[] }

const SUBJECTS: Subject[] = [
  {
    id: 'applyDiscount', label: 'applyDiscount', kind: 'symbol', dial: 0.55,
    facts: [
      { stratum: 'requirements', text: 'satisfies REQ-142 · validated ✓', detail: 'requirement↔code · enforced', conf: 1.0, prov: 'Parsed' },
      { stratum: 'blast', text: '3 transitive dependents', detail: 'checkout · cartTotal · api/price', conf: 1.0, prov: 'SCIP' },
      { stratum: 'blast', text: 'injected: emits wicked.shop.order.placed → 2 consumers', detail: 'event→consumer edge · grep never sees this', conf: 1.0, prov: 'Injected' },
      { stratum: 'blast', text: 'referralFlow → applyDiscount', detail: 'tag-scan guess · cross-file unverified', conf: 0.3, prov: 'Tags' },
      { stratum: 'infra', text: 'reads dataset PRICING.TBL via api/price', detail: 'code↔dataset edge', conf: 0.85, prov: 'ImportMap' },
      { stratum: 'history', text: '4 commits · last changed 2026-06', detail: 'per-file git provenance', conf: 1.0, prov: 'git' },
      { stratum: 'annotations', text: 'assumption: max one coupon per cart', detail: 'advisory · survives re-index', conf: 0.7, prov: 'annotation', advisory: true },
    ],
  },
  {
    id: 'REQ-142', label: 'REQ-142', kind: 'requirement', dial: 0.90,
    facts: [
      { stratum: 'requirements', text: '2 symbols satisfy REQ-142', detail: 'validateCoupon ✓ · applyDiscount ⋯ unvalidated', conf: 1.0, prov: 'Parsed' },
      { stratum: 'blast', text: 'blast-radius of implementers: 3 dependents', detail: 'checkout · cartTotal · api/price', conf: 1.0, prov: 'SCIP' },
      { stratum: 'blast', text: 'candidate impl: legacyDiscount()', detail: 'import-map heuristic · not confirmed', conf: 0.6, prov: 'ImportMap' },
      { stratum: 'infra', text: 'governed by rule-set PricingPolicy', detail: 'ODM ruleset↔code edge · same graph', conf: 0.9, prov: 'Parsed' },
      { stratum: 'history', text: 'validated flag flipped 2026-05', detail: 'read-only edge-history log', conf: 1.0, prov: 'git' },
      { stratum: 'annotations', text: 'question: does BOGO count as a coupon?', detail: 'advisory · open', conf: 0.5, prov: 'annotation', advisory: true },
    ],
  },
  {
    id: 'PAYROLL.JCL', label: 'PAYROLL.JCL', kind: 'JCL step', dial: 0.70,
    facts: [
      { stratum: 'infra', text: 'EXEC PGM=PAYCALC uses PAYROLL.MASTER', detail: 'JCL step↔dataset edge', conf: 1.0, prov: 'Parsed' },
      { stratum: 'infra', text: 'RACF profile PAY.** protects PAYROLL.MASTER', detail: 'cross-domain: RACF↔dataset · one query', conf: 1.0, prov: 'Parsed' },
      { stratum: 'infra', text: 'injected: command:deploy → deploy-agent', detail: 'command→agent edge · grep never sees this', conf: 1.0, prov: 'Injected' },
      { stratum: 'blast', text: '2 callers submit this job', detail: 'scheduler.ts · nightly.sh', conf: 0.85, prov: 'ImportMap' },
      { stratum: 'history', text: 'drift: live RACF ≠ iac since 2026-05', detail: 'graph diff · resource identity', conf: 0.8, prov: 'Collector' },
      { stratum: 'requirements', text: 'supports REQ-207 · unvalidated ⋯', conf: 0.6, prov: 'Parsed' },
      { stratum: 'annotations', text: 'note: dataset last drilled 2026-05', conf: 0.6, prov: 'annotation', advisory: true },
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
  const [threshold, setThreshold] = useState(SUBJECTS[0].dial)
  const [driving, setDriving] = useState(false)

  const subject = SUBJECTS[subjectIdx]
  const live = subject.facts.filter(f => f.conf >= threshold)
  const liveStrata = new Set(live.map(f => f.stratum))

  // auto-demo: dwell ~5s on each subject (each subject is a tab), then advance
  // to the next and cycle. Clicking a subject or driving the dial pins it and
  // stops the auto-progression.
  useEffect(() => {
    if (driving || reduced) return
    const t = setInterval(() => setSubjectIdx(i => (i + 1) % SUBJECTS.length), 5000)
    return () => clearInterval(t)
  }, [driving, reduced])

  // The confidence dial SWEEPS: when the tab advances, the needle glides from its
  // current value to the new subject's read-out cutoff — so the dial visibly moves
  // between subjects instead of sitting still. Held facts filter live as it travels.
  const thresholdRef = useRef(threshold)
  thresholdRef.current = threshold
  const rafRef = useRef<number | undefined>(undefined)
  useEffect(() => {
    if (driving) return
    const target = SUBJECTS[subjectIdx].dial
    if (reduced) { setThreshold(target); return } // no animation, but still land on the value
    const start = thresholdRef.current
    const t0 = performance.now()
    const dur = 900
    const ease = (x: number) => 1 - Math.pow(1 - x, 3)
    const tick = (now: number) => {
      const p = Math.min(1, (now - t0) / dur)
      setThreshold(+(start + (target - start) * ease(p)).toFixed(4))
      if (p < 1) rafRef.current = requestAnimationFrame(tick)
    }
    rafRef.current = requestAnimationFrame(tick)
    return () => { if (rafRef.current) cancelAnimationFrame(rafRef.current) }
  }, [subjectIdx, driving, reduced])

  const takeControl = () => setDriving(true)

  return (
    <Section id="query" solid>
      <div className="max-w-6xl mx-auto w-full">
        <div className="mb-2.5 w-full text-left flex flex-wrap items-end justify-between gap-4">
          <div>
            <span className="kicker">Query the substrate</span>
            <h2 className="mt-1.5 font-display text-2xl sm:text-[1.95rem] font-black text-ink w-full leading-[0.98]">
              One question. One dossier. Assembled live across all five strata.
            </h2>
            <p className="mt-1.5 text-sm text-ink w-full font-sans leading-tight max-w-2xl">
              Watch it read itself — it steps through each subject and the confidence dial sweeps to that subject's
              cutoff. Click any subject to pin it, then drive the dial: push to{' '}
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
        <div className="flex flex-wrap gap-2 mb-2">
          {SUBJECTS.map((s, i) => {
            const on = i === subjectIdx
            return (
              <button
                key={s.id}
                onClick={() => { takeControl(); setSubjectIdx(i); setThreshold(s.dial) }}
                className="text-left rounded-xl px-4 py-1.5 transition-all"
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
                {on && !driving && !reduced && (
                  <div className="tab-progress" key={subjectIdx}><span /></div>
                )}
              </button>
            )
          })}
        </div>

        <div className="grid lg:grid-cols-[280px_1fr] gap-3">
          {/* Left — confidence dial + core column */}
          <div className="rock-panel p-4 flex flex-col gap-3.5">
            <div>
              <div className="flex items-baseline justify-between mb-2.5">
                <span className="kicker">Confidence dial</span>
                <span className="dial-readout font-mono text-xl font-black tabular-nums" style={{ color: 'var(--accent)' }}>
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
              <p className="mt-2.5 font-mono text-[0.62rem] text-ink leading-5">
                {live.length} of {subject.facts.length} facts above cutoff · {liveStrata.size} of 5 strata live
              </p>
            </div>

            {/* the drill core: which strata have a live fact */}
            <div>
              <span className="kicker">Core column</span>
              <div className="relative mt-2 pl-3">
                <div className="seam-line absolute top-1 bottom-1 left-0" />
                {STRATA.map(s => {
                  const on = liveStrata.has(s.id)
                  return (
                    <div key={s.id} className="flex items-center gap-3 py-1">
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
            <div className="flex items-center justify-between px-5 py-2 border-b border-hairline-strong">
              <span className="font-mono text-sm text-ink font-semibold">{subject.label}</span>
              <span className="depth">substrate.query({subject.kind}) → 1 dossier · 5 strata</span>
            </div>
            <div className="divide-y divide-hairline">
              {STRATA.map(s => {
                const facts = subject.facts.filter(f => f.stratum === s.id)
                if (facts.length === 0) return null
                return (
                  <div key={s.id} className="flex gap-4 px-5 py-1">
                    <div className="w-28 shrink-0 pt-0.5">
                      <div className="font-mono text-[0.6rem] text-faint">{s.no}</div>
                      <div className="font-display font-black text-ink text-sm leading-tight" style={{ fontStretch: '106%' }}>{s.name}</div>
                      <div className="depth mt-0.5">{s.depth}</div>
                    </div>
                    <div className="flex-1 flex flex-col gap-1 min-w-0">
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
  const [pinned, setPinned] = useState(false)

  useEffect(() => {
    if (reduced || pinned) return
    const t = setInterval(() => setActive(a => (a + 1) % STRATA.length), 1900)
    return () => clearInterval(t)
  }, [reduced, pinned])

  return (
    <Section id="strata">
      <div className="max-w-6xl mx-auto w-full">
        <TopHead
          kicker="The five strata"
          title={<>Five bands. One substrate. <span style={{ color: 'var(--accent)' }}>One symbol identity.</span></>}
        >
          <p className="max-w-3xl">
            Not a pile of scanners — a single continuous body, cut into bands. Each keyed to the same stable{' '}
            <span className="font-mono text-sm font-semibold">(scheme, qualified-name)</span> that survives reformatting,
            moves, and re-index.
          </p>
        </TopHead>

        <div className="mb-3">
          <button
            className="demo-pill"
            data-live={String(!pinned)}
            onClick={() => setPinned(p => !p)}
            aria-label={pinned ? 'Resume the auto scan' : 'Pause and explore the bands yourself'}
          >
            <span className="dot" />
            {pinned ? 'Pinned · click to resume scan' : 'Auto-scanning · click a band to pin'}
          </button>
        </div>

        <div className="rock-panel p-0">
          <div className="relative">
            <div className="seam-line absolute top-4 bottom-4" style={{ left: '22%' }} />
            {STRATA.map((s, i) => {
              const on = i === active
              return (
                <div
                  key={s.id}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => { setPinned(true); setActive(i) }}
                  className="relative flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-6 px-6 py-3.5 border-b border-hairline last:border-b-0 transition-all duration-300 cursor-pointer"
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
        <p className="mt-4 font-mono text-xs text-faint">
          Requirements↔implementation · blast-radius · infra + policy · operational history · typed annotations — one binary, every fact stamped with confidence and provenance.
        </p>
      </div>
    </Section>
  )
}

// ── 3b · THE FULL TOOLFACE — every tool + skill an agent can call ───────────────
// Grounded to v0.13.1. Tools: crates/wicked-estate-mcp/src/lib.rs (all_tools() +
// memory/knowledge schemas), README.md §MCP. Skills: crates/*/skills/*/SKILL.md.
type ToolDomain = { no: string; name: string; note: string; tools: { name: string; purpose: string }[] }

const TOOL_DOMAINS: ToolDomain[] = [
  {
    no: '01', name: 'Graph', note: '10 estate tools',
    tools: [
      { name: 'SearchEntity',   purpose: 'Find symbols by name or kind; optional source inline.' },
      { name: 'RetrieveEntity', purpose: 'One symbol’s full dossier — callers, edges, annotations, requirement.' },
      { name: 'TraverseGraph',  purpose: 'Bounded walk out over calls and imports.' },
      { name: 'BlastRadius',    purpose: 'Every dependent — what breaks if you change it.' },
      { name: 'Lineage',        purpose: 'The dependency chain a symbol rests on, transitively.' },
      { name: 'RankHotspots',   purpose: 'Most-connected symbols by PageRank — where to start reading.' },
      { name: 'Communities',    purpose: 'Clusters the graph into modules.' },
      { name: 'ContextBundle',  purpose: 'Scoped, prompt-ready context pack for a symbol.' },
      { name: 'FetchContent',   purpose: 'The stored source for a symbol or file.' },
      { name: 'RulesInventory', purpose: 'Rules engines (ODM · DMN · Drools · CLIPS) + the code that calls them.' },
    ],
  },
  {
    no: '02', name: 'Memory', note: '6 memory tools',
    tools: [
      { name: 'memory.capture',  purpose: 'Capture a memory node (episodic / semantic / procedural / archival).' },
      { name: 'memory.recall',   purpose: 'Token-budgeted recall relevant to a query in scope.' },
      { name: 'memory.learn',    purpose: 'Store a semantic fact and link it to code symbols atomically.' },
      { name: 'memory.reflect',  purpose: 'Distil episodic memories in a scope into semantic facts.' },
      { name: 'memory.coverage', purpose: 'Node counts by tier and kind.' },
      { name: 'memory.erase',    purpose: 'Hard-delete every memory under a scope prefix.' },
    ],
  },
  {
    no: '03', name: 'Knowledge', note: '7 knowledge tools',
    tools: [
      { name: 'knowledge.ingest',            purpose: 'Ingest a document as a doc + retrievable chunk nodes.' },
      { name: 'knowledge.write',             purpose: 'Write one node (doc / section / chunk / concept).' },
      { name: 'knowledge.relate',            purpose: 'Add a typed, confidence-scored relation between nodes.' },
      { name: 'knowledge.recall',            purpose: 'Hybrid FTS + vector recall, RRF-fused.' },
      { name: 'knowledge.relate_code',       purpose: 'Link a knowledge node to estate code symbols.' },
      { name: 'knowledge.recall_about_code', purpose: 'Recall knowledge linked to given code symbols.' },
      { name: 'knowledge.coverage',          purpose: 'Node counts per class.' },
    ],
  },
]

// Agent skills shipped in-repo (crates/*/skills/*/SKILL.md) — the playbooks an
// agent runs against the tools above.
const AGENT_SKILLS: { name: string; purpose: string }[] = [
  { name: 'codebase-expedition',  purpose: 'Hotspot-first tour: RankHotspots → TraverseGraph → FetchContent.' },
  { name: 'knowledge-ingest',     purpose: 'Chunk and ingest a document into the knowledge base.' },
  { name: 'cited-answer',         purpose: 'Answer with a grounded, cited slice — never from model memory.' },
  { name: 'ontology-expedition',  purpose: 'Connect concepts with typed relations — the bar over a flat brain.' },
  { name: 'knowledge-curation',   purpose: 'Dedup as the base grows — collapse-but-surface, never delete.' },
  { name: 'gap-hunting',          purpose: 'Turn recall misses into ingest tasks — close the loop.' },
]

// Cross-cutting capabilities, each grounded in a repo path.
const CAPABILITIES: string[] = [
  'Injected edges · event→consumer · command→agent',
  'Every edge: confidence + provenance + resolved_by',
  '7 resolution tiers · Parsed → SCIP → LSP',
  'Rules engines in the same graph · ODM · DMN · Drools',
  'Requirement ↔ code traceability',
  'Typed annotations · survive re-index',
  'SQLite by default · Postgres behind one flag',
  '100+ languages as data — a row + a query file',
]

function FullToolface() {
  return (
    <Section id="toolface" solid>
      <div className="max-w-6xl mx-auto w-full">
        <div className="mb-2.5 w-full text-left">
          <span className="kicker">Everything an agent can call</span>
          <h2 className="mt-1.5 font-display text-2xl sm:text-[1.95rem] font-black text-ink leading-[0.98]">
            23 MCP tools. 6 agent skills. <span style={{ color: 'var(--accent)' }}>One binary.</span>
          </h2>
          <p className="mt-1.5 text-sm text-muted font-sans leading-tight max-w-3xl">
            Not one “search” tool bolted onto a repo — the full MCP surface an agent can call, plus the skills
            (playbooks) that drive them. The memory and knowledge domains pair with{' '}
            <span className="text-ink">wicked-brain</span>, where the memory + code-graph proper live; the estate
            domain is the live technical environment. Grounded to v0.13.1.
          </p>
        </div>

        {/* the three MCP domains — every tool name + one-line purpose */}
        <div className="grid lg:grid-cols-3 gap-2.5">
          {TOOL_DOMAINS.map(d => (
            <div key={d.name} className="rock-panel p-0">
              <div className="flex items-center gap-2.5 px-4 py-1.5 border-b border-hairline-strong">
                <span className="font-display font-black text-base tabular-nums" style={{ fontStretch: '108%', color: 'var(--accent)' }}>{d.no}</span>
                <span className="font-display font-black text-ink text-[0.95rem] flex-1" style={{ fontStretch: '108%' }}>{d.name}</span>
                <span className="tag tag-accent">{d.note}</span>
              </div>
              <div className="divide-y divide-hairline">
                {d.tools.map(t => (
                  <div key={t.name} className="px-4 py-[2px] flex items-baseline gap-2">
                    <span className="font-mono text-[0.7rem] font-semibold text-ink shrink-0">{t.name}</span>
                    <span className="depth flex-1 min-w-0 text-right" style={{ whiteSpace: 'normal', lineHeight: 1.25 }}>{t.purpose}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* the agent skills — the playbooks that ship with estate */}
        <div className="mt-2 rock-panel p-0">
          <div className="flex items-center gap-3 px-4 py-2 border-b border-hairline-strong">
            <span className="kicker">Agent skills · in-repo playbooks</span>
            <span className="tag ml-auto">6 skills</span>
          </div>
          <div className="grid sm:grid-cols-2 lg:grid-cols-3 divide-y sm:divide-y-0 divide-hairline">
            {AGENT_SKILLS.map(s => (
              <div key={s.name} className="px-4 py-[7px] flex items-baseline gap-2" style={{ boxShadow: 'inset 0 0 0 0.5px var(--hairline)' }}>
                <span className="font-mono text-[0.7rem] font-semibold text-ink shrink-0">{s.name}</span>
                <span className="depth flex-1 min-w-0 text-right" style={{ whiteSpace: 'normal', lineHeight: 1.25 }}>{s.purpose}</span>
              </div>
            ))}
          </div>
        </div>

        {/* cross-cutting capabilities most agents never know estate has */}
        <div className="mt-2 flex flex-wrap gap-1.5">
          {CAPABILITIES.map(c => <span key={c} className="tag">{c}</span>)}
        </div>
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
          kicker="One backend, solo or shared"
          title={<>SQLite by default. <span style={{ color: 'var(--accent)' }}>One flag</span> to a shared team backend.</>}
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

// ── 6 · WHERE ESTATE SITS IN THE LOOP — cycle the four verbs, Equip is home ──────
// The one canonical wicked visual is a LOOP, not a ranked stack:
//   intent → Steer → Equip → (harness) → Verify · Govern → record → next run,
//   all under human authority. bus is the fabric beneath; interactive a surface beside.
type LoopStep = { id: string; verb: string; here?: boolean; doing: string; members: { name: string; note: string }[] }
const LOOP: LoopStep[] = [
  {
    id: 'steer', verb: 'Steer',
    doing: `Steering before execution — reads each prompt's work-shape + risk and applies the right rigor, plus the capabilities a planner-executor can't do alone.`,
    members: [{ name: 'garden', note: 'the Steer product' }],
  },
  {
    id: 'equip', verb: 'Equip', here: true,
    doing: `Your live technical environment, queryable — requirements↔implementation, blast-radius, infra + policy relationships, operational history. Memory + code-graph with provenance — knowledge the agent can search, challenge, correct, and trace to its source.`,
    members: [{ name: 'estate', note: 'requirements↔impl · blast-radius · infra + policy · history' }, { name: 'brain', note: 'memory + code-graph with provenance' }],
  },
  {
    id: 'verify', verb: 'Verify',
    doing: `No agent grades its own homework — an enforced wall between the agent that runs the tests and the one that judges them.`,
    members: [{ name: 'testing', note: 'the acceptance gate' }],
  },
  {
    id: 'govern', verb: 'Govern',
    doing: `The engine that makes "done" a mechanism, not a claim — workflow-as-data, dual gates, state re-derived from evidence. The control room for governed agent delivery — drive, gate, and audit the work; the human stays in command.`,
    members: [{ name: 'core', note: 'the engine' }, { name: 'crew', note: 'the control room' }],
  },
]

function FamilyLoop() {
  const reduced = useReducedMotion()
  const [active, setActive] = useState(1) // start on Equip (estate’s home)
  const [pinned, setPinned] = useState(false)

  useEffect(() => {
    if (reduced || pinned) return
    const t = setInterval(() => setActive(a => (a + 1) % LOOP.length), 2600)
    return () => clearInterval(t)
  }, [reduced, pinned])

  const current = LOOP[active]

  return (
    <Section id="foundation">
      <div className="max-w-5xl mx-auto w-full">
        <TopHead
          kicker="Where estate sits in the loop"
          title={<>Estate is where the loop gets <span style={{ color: 'var(--accent)' }}>equipped.</span></>}
        >
          <p className="max-w-3xl">
            One loop, four verbs, under human authority: <span className="font-semibold">intent → Steer → Equip → (your harness) → Verify · Govern → record</span>,
            and the record feeds the next run. estate is one half of <span className="font-semibold">Equip</span> — the technical environment;{' '}
            <span className="font-mono text-sm">wicked-brain</span> is the other.
          </p>
        </TopHead>

        <div className="grid lg:grid-cols-[0.9fr_1.1fr] gap-8 items-center">
          {/* left — what the active verb does */}
          <div className="text-left">
            <span className="kicker" style={{ color: current.here ? 'var(--accent)' : 'var(--muted)' }}>{current.verb}{current.here ? ' · you are here' : ''}</span>
            <p className="mt-3 text-lg text-ink font-sans leading-relaxed min-h-[7.5rem]">{current.doing}</p>
            <div className="mt-4 flex gap-1.5">
              {LOOP.map((l, i) => (
                <button key={l.id} onClick={() => { setPinned(true); setActive(i) }} aria-label={l.verb}
                  className="h-1.5 rounded-full transition-all"
                  style={{ width: i === active ? 24 : 8, background: i === active ? 'var(--accent)' : 'var(--hairline-strong)' }} />
              ))}
              <button
                onClick={() => setPinned(!pinned)}
                className="depth depth-toggle ml-2"
                aria-label={pinned ? 'Resume cycling the loop' : 'Pin the loop'}
              >
                {pinned ? 'pinned (click to cycle)' : 'cycling the loop'}
              </button>
            </div>
          </div>

          {/* right — the four verbs; a left pointer marks the active one */}
          <div className="rock-panel p-0 overflow-hidden">
            {LOOP.map((l, i) => {
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
                  <div className="flex-1 px-4 py-4" style={{ background: l.here ? 'color-mix(in oklab, var(--accent) 7%, transparent)' : 'transparent' }}>
                    <div className="flex items-center gap-3 mb-2 flex-wrap">
                      <span className="kicker" style={{ color: on ? 'var(--accent)' : 'var(--muted)' }}>{l.verb}</span>
                      {l.here && <span className="tag tag-accent">estate lives here</span>}
                    </div>
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
          wicked-bus is the durable fabric beneath the loop · wicked-interactive is a creative surface beside it · every run stays under human authority — approve, redirect, pause, cancel.
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
      <FullToolface />
      <ProvenanceSeam />
      <Storage />
      <FamilyLoop />
      <GetStarted />
    </main>
  )
}
