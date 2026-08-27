import { useState, useEffect, useRef } from 'react'

/* ────────────────────────────────────────────────────────────────────────────
   wicked-estate — THE FOUNDATION · the system of record.

   ROLE · estate is the foundation plane of the wicked platform: code graph +
   memory + knowledge in ONE binary (23 MCP tools across 3 domains). It is the
   record every agent reads before it acts and writes after it's done — the
   capability plane reads and writes it through its contract, never around it.

   CONCEPT · "read the record." One durable body you read like a geologist
   reads a drill core: five bands (code graph · injected edges · memory ·
   knowledge · provenance), every band keyed to one stable symbol identity,
   every fact stamped with confidence + provenance. The signature motion is a
   DRILL that reads the core — sections demo themselves before you touch them.

   Grounded to v0.14.4 (crates.io): 102 wired tree-sitter languages
   (languages-as-data — 113 in the manifest), ExtraEdge TOML rules injecting
   the edges grep never sees (event→consumer, command→agent), every edge
   {confidence, provenance, resolved_by}, memory scopes with subtree
   recall/erase, hybrid RRF retrieval with a public parity bench
   (overall r@10 0.849 vs 0.437 for the FTS system it replaced), SQLite by
   default with the WICKED_RUNTIME team profile seam to shared Postgres.
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

// tracks whether the viewport is phone-sized, so components can swap the tuned
// desktop layout for a legible stacked/simplified mobile treatment. Initial state
// is `false` (matches the SSR / first-hydration render — desktop), then the effect
// corrects it on mount, so there is no hydration mismatch.
function useIsMobile(maxWidth = 760) {
  const [isMobile, setIsMobile] = useState(false)
  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return
    const mq = window.matchMedia(`(max-width: ${maxWidth}px)`)
    const on = () => setIsMobile(mq.matches)
    on()
    // Safari < 14 / older iOS lack addEventListener on MediaQueryList.
    if (mq.addEventListener) {
      mq.addEventListener('change', on)
      return () => mq.removeEventListener('change', on)
    }
    mq.addListener(on)
    return () => mq.removeListener(on)
  }, [maxWidth])
  return isMobile
}

// ── Strata metadata: the five bands of the record ───────────────────────────
type StratumId = 'graph' | 'injected' | 'memory' | 'knowledge' | 'provenance'

const STRATA: { id: StratumId; no: string; name: string; depth: string; tools: string; copy: string }[] = [
  { id: 'graph',      no: '01', name: 'Code graph',     depth: '−0.0m',  tools: '102 languages',
    copy: 'Symbols, calls, imports, heritage — tree-sitter extraction across 102 wired languages. Languages are data, not code: a new one is a manifest row + a query file, zero core change.' },
  { id: 'injected',   no: '02', name: 'Injected edges', depth: '−4.2m',  tools: 'ExtraEdge TOML',
    copy: 'The relationships grep never sees — event→consumer, command→agent — injected by plain-TOML ExtraEdge rules. A new domain edge type is a rule block, zero Rust; blast-radius crosses the event-bus boundary.' },
  { id: 'memory',     no: '03', name: 'Memory',         depth: '−7.8m',  tools: 'scoped',
    copy: 'Episodic, semantic, procedural — captured under scopes, recalled on a token budget, distilled by reflection. Erasable by subtree: one call hard-deletes everything under a scope prefix. Erasability is governance.' },
  { id: 'knowledge',  no: '04', name: 'Knowledge',      depth: '−11.5m', tools: 'RRF-fused',
    copy: 'Docs and wiki as retrievable nodes with typed, confidence-scored relations — linked to the code symbols they describe. Hybrid FTS + vector recall, RRF-fused, benchmarked in the open.' },
  { id: 'provenance', no: '05', name: 'Provenance',     depth: '−14.0m', tools: 'every edge',
    copy: 'Every edge carries {confidence, provenance, resolved_by}. When tiers disagree, the highest wins and the losers are superseded — a 0.3 tag-scan guess is never handed to an agent as a 1.0 fact.' },
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

// full-container-width, left-aligned header for sections whose title sits ON TOP of content.
// The type scale here is load-bearing for section height: every .strata section has to fit
// 100svh − topbar, and this head is the first thing every one of them spends. It was
// 2.7rem/mb-8/mt-4 and cost #storage 277px of a 636px budget — see tests/e2e/section-fits.spec.ts.
function TopHead({ kicker, title, children }: { kicker: string; title: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="mb-5 w-full text-left">
      <span className="kicker">{kicker}</span>
      <h2 className="mt-2.5 font-display text-2xl sm:text-[2.15rem] font-black text-ink w-full leading-[1.0]">{title}</h2>
      {children && <div className="mt-2.5 text-[0.95rem] text-ink w-full font-sans leading-[1.55] max-w-none">{children}</div>}
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
    <Section className="strata--hero !pt-20 overflow-hidden">
      <div className="max-w-6xl mx-auto w-full grid lg:grid-cols-[1.08fr_0.92fr] gap-12 items-center">
        {/* Left — the thesis, committed in sentence one */}
        <div className="text-left">
          <span className="kicker">wicked-estate · the foundation · v0.14.4 · crates.io</span>
          <h1 className="mt-3 font-display font-black text-ink text-[clamp(2.1rem,10vw,3rem)] sm:text-5xl lg:text-[3.5rem] leading-[0.94]" style={{ fontStretch: '112%' }}>
            Code graph.<br />Memory.<br />Knowledge.<br />
            <span style={{ color: 'var(--accent)' }}>One binary.</span>
          </h1>
          <p className="mt-4 text-[1.05rem] text-muted leading-[1.5] max-w-xl font-sans">
            The <span className="text-ink">system of record</span> for your codebase — one local-first MCP server,{' '}
            <span className="text-ink">23 tools across 3 domains</span>. What breaks if you change it, the decision
            behind it, the doc that explains it — including the{' '}
            <span className="text-ink">injected edges grep never sees</span>, every fact stamped with confidence and
            provenance.
          </p>
          <p className="mt-2.5 text-sm text-muted leading-[1.5] max-w-xl font-sans">
            Zero infrastructure by default — one SQLite file, nothing leaves your box. And it earned the job: its
            hybrid retrieval is <span className="text-ink">parity-benchmarked in the open</span> against the
            dedicated search system it replaced.
          </p>
          <div className="mt-4 flex flex-col sm:flex-row gap-3">
            <a href="#query" className="btn-primary">Read the record ↓</a>
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
            One continuous record — five bands, one symbol identity.
          </p>
        </div>
      </div>
    </Section>
  )
}

// ── 2 · THE AGENT'S IDE — estate is the IDE the agent edits code in ─────────────
// The centerpiece. The agent right-clicks a symbol → a context menu of real IDE
// commands → one runs → the result lands in a tabbed dock (Code intelligence ·
// Memory · Knowledge). Clicking a class shows all its details at once. The dock
// proves the record gives an agent EVERYTHING — code intelligence (callers /
// blast-radius / requirement / definition), recalled Memory (decisions · patterns ·
// gotchas, with scope + salience + provenance) and Knowledge (the wiki, RRF-fused).
// Every result carries confidence + provenance; the dial gates the low-confidence /
// low-salience ones. The auto-play is a storyboard of these gestures — a visible
// cursor + the menu opening — driven by an IntersectionObserver; any interaction
// takes control (driving=true) and stops the demo.

type Prov = 'Parsed' | 'SCIP' | 'Injected' | 'ImportMap' | 'Tags' | 'git' | 'Memory' | 'Knowledge' | 'annotation'
interface Fact { text: string; detail?: string; conf: number; prov: Prov; advisory?: boolean }
type SymId = 'PricingService' | 'applyDiscount'
type DockTab = 'code' | 'memory' | 'knowledge'
// the record's answer to one code-intelligence command
interface Peek { title: string; sub: string; facts: Fact[]; wiki?: string }
// a context-menu / command-palette command and where its result lands
interface Command { id: string; label: string; note?: string; lands: DockTab; peek?: Peek; memId?: string }
// a token in the code; `sym` set => it's a clickable symbol
interface Token { t: string; sym?: SymId; k?: string }
// one recalled memory node — decision / pattern / gotcha
interface Memory { id: string; kind: 'decision' | 'pattern' | 'gotcha'; text: string; scope: string; salience: number; prov: string; because: string; superseded?: boolean }
// one recalled knowledge (wiki) node
interface Know { id: string; title: string; section: string; detail: string; conf: number }

// The working file shown in the editor (each line is a token list; symbols are buttons).
const CODE: Token[][] = [
  [{ t: 'export ', k: 'kw' }, { t: 'class ', k: 'kw' }, { t: 'PricingService', sym: 'PricingService', k: 'nm' }, { t: ' {' }],
  [{ t: '  ' }, { t: 'applyDiscount', sym: 'applyDiscount', k: 'fn' }, { t: '(cart, coupon) {' }],
  [{ t: '    ' }, { t: 'const ', k: 'kw' }, { t: 'price = ' }, { t: 'cartTotal', k: 'fn' }, { t: '(cart)' }],
  [{ t: '    ' }, { t: 'return ', k: 'kw' }, { t: 'clamp', k: 'fn' }, { t: '(price - coupon.value, ' }, { t: '0', k: 'nm' }, { t: ')' }],
  [{ t: '  }' }],
  [{ t: '}' }],
]

// The IDE commands (context menu = command palette). Most land code-intelligence results
// in the Code tab; "Recall decisions" lands in Memory, "Search knowledge" in Knowledge.
const COMMANDS: Command[] = [
  { id: 'definition', label: 'Peek definition', note: 'F12', lands: 'code', peek: {
    title: 'Peek definition', sub: 'PricingService.applyDiscount',
    facts: [
      { text: 'applyDiscount(cart, coupon) → number', detail: 'checkout/PricingService.ts:2 · exported', conf: 1.0, prov: 'Parsed' },
      { text: 'returns clamp(price − coupon.value, 0)', detail: 'pure · does not mutate cart', conf: 1.0, prov: 'SCIP' },
    ],
  } },
  { id: 'callers', label: 'Find all callers', note: '⇧F12', lands: 'code', peek: {
    title: 'Find all callers', sub: 'applyDiscount · 3 found',
    facts: [
      { text: 'checkout() → applyDiscount', detail: 'checkout/Checkout.ts:41', conf: 1.0, prov: 'SCIP' },
      { text: 'api/price route → applyDiscount', detail: 'routes/price.ts:18', conf: 1.0, prov: 'SCIP' },
      { text: 'referralFlow → applyDiscount', detail: 'tag-scan guess · cross-file, unverified', conf: 0.3, prov: 'Tags' },
    ],
  } },
  { id: 'references', label: 'Find all references', note: 'blast radius', lands: 'code', peek: {
    title: 'Find all references · blast radius', sub: 'what breaks if you change it',
    facts: [
      { text: '3 transitive dependents', detail: 'checkout · cartTotal · api/price', conf: 1.0, prov: 'SCIP' },
      { text: 'emits wicked.shop.order.placed → 2 consumers', detail: 'injected by an ExtraEdge TOML rule · grep never sees this', conf: 1.0, prov: 'Injected' },
      { text: 'referralFlow → applyDiscount', detail: 'tag-scan guess · cross-file, unverified', conf: 0.3, prov: 'Tags' },
    ],
  } },
  { id: 'requirement', label: 'Go to requirement', note: '⌘R', lands: 'code', peek: {
    title: 'Go to requirement', sub: 'requirement ↔ implementation', wiki: '[[Pricing Rules]] §Discounts',
    facts: [
      { text: 'satisfies REQ-142 · validated ✓', detail: '“coupons never exceed the cart total” · enforced', conf: 1.0, prov: 'Parsed' },
      { text: 'validated flag flipped 2026-05', detail: 'read-only edge-history log', conf: 1.0, prov: 'git' },
    ],
  } },
  { id: 'decisions', label: 'Recall decisions', note: '⌘K', lands: 'memory', memId: 'm-nostack' },
  { id: 'policy', label: 'Show governing policy', note: '⌘I', lands: 'code', peek: {
    title: 'Show governing policy', sub: 'infra + policy, cross-domain',
    facts: [
      { text: 'governed by rule-set PricingPolicy', detail: 'ODM ruleset ↔ code edge · same graph', conf: 0.9, prov: 'Parsed' },
      { text: 'reads dataset PRICING.TBL via api/price', detail: 'code ↔ dataset edge', conf: 0.85, prov: 'ImportMap' },
    ],
  } },
  { id: 'annotations', label: 'Annotations', note: '⌘/', lands: 'code', peek: {
    title: 'Annotations', sub: 'typed notes · survive re-index',
    facts: [
      { text: 'assumption: max one coupon per cart', detail: 'advisory · survives re-index', conf: 0.7, prov: 'annotation', advisory: true },
      { text: 'note: pricing last audited 2026-05', detail: 'typed note', conf: 0.6, prov: 'annotation', advisory: true },
    ],
  } },
  { id: 'knowledge', label: 'Search knowledge', note: '⌘⇧F', lands: 'knowledge' },
]

// Clicking the class opens the "all details" peek — everything the record knows, at once.
const DETAILS: Peek = {
  title: 'PricingService — all details', sub: 'everything the record knows, at once', wiki: '[[Pricing Rules]] §Discounts',
  facts: [
    { text: 'class PricingService · 1 public method', detail: 'checkout/PricingService.ts:1', conf: 1.0, prov: 'Parsed' },
    { text: 'satisfies REQ-142 · validated ✓', detail: 'coupons never exceed the cart total', conf: 1.0, prov: 'Parsed' },
    { text: '3 callers · 3 transitive dependents', detail: 'checkout · api/price · cartTotal', conf: 1.0, prov: 'SCIP' },
    { text: 'emits wicked.shop.order.placed → 2 consumers', detail: 'ExtraEdge rule · grep never sees this', conf: 1.0, prov: 'Injected' },
    { text: 'decision: coupons never stack', detail: 'recalled from memory · scope project:acme', conf: 0.92, prov: 'Memory' },
    { text: 'governed by rule-set PricingPolicy', detail: 'ODM ruleset · reads PRICING.TBL', conf: 0.9, prov: 'Parsed' },
    { text: 'assumption: max one coupon per cart', detail: 'advisory · survives re-index', conf: 0.7, prov: 'annotation', advisory: true },
  ],
}

// The Memory panel — recalled memory relevant to the code in view. Salience acts
// as the confidence the dial gates; the superseded decision falls below the default cutoff.
const MEMORIES: Memory[] = [
  { id: 'm-nostack', kind: 'decision', text: 'Coupons never stack — one per cart', scope: 'project:acme', salience: 0.92, prov: 'episodic · spike 2026-06', because: 'applyDiscount' },
  { id: 'm-clamp', kind: 'pattern', text: 'Discount math clamps at 0 — never a negative total', scope: 'project:acme', salience: 0.8, prov: 'semantic · reflected 2026-05', because: 'applyDiscount' },
  { id: 'm-tz', kind: 'gotcha', text: 'PRICING.TBL is timezone-naive — normalize before compare', scope: 'project:acme', salience: 0.68, prov: 'episodic · spike 2026-04', because: 'api/price' },
  { id: 'm-old', kind: 'decision', text: 'Stack up to 2 coupons — superseded by REQ-142', scope: 'project:acme', salience: 0.34, prov: 'episodic · spike 2025-11', because: 'applyDiscount', superseded: true },
]

// A touch of Knowledge — the wiki, hybrid FTS + vector, RRF-fused.
const KNOWLEDGE: Know[] = [
  { id: 'k-rules', title: 'Pricing Rules', section: '§Discounts', detail: 'coupon eligibility + stacking policy · hybrid FTS+vector, RRF-fused', conf: 0.9 },
  { id: 'k-flow', title: 'Checkout Flow', section: '§Totals', detail: 'where applyDiscount sits in the order lifecycle', conf: 0.85 },
  { id: 'k-adr', title: 'ADR-014 Coupon Policy', section: '§Decision', detail: 'why stacking was dropped — links REQ-142', conf: 0.88 },
]

// The auto-play storyboard: real IDE gestures, ~2.6s each. A visible cursor moves to the
// target; a right-click opens the menu and one command runs; a class click opens details.
// The result lands in the matching dock tab — so the tour shows code · memory · knowledge.
interface Gesture { target: SymId; kind: 'menu' | 'click'; run?: string }
const STORYBOARD: Gesture[] = [
  { target: 'applyDiscount', kind: 'menu', run: 'callers' },
  { target: 'PricingService', kind: 'click' },
  { target: 'applyDiscount', kind: 'menu', run: 'requirement' },
  { target: 'applyDiscount', kind: 'menu', run: 'decisions' },
  { target: 'applyDiscount', kind: 'menu', run: 'references' },
  { target: 'applyDiscount', kind: 'menu', run: 'knowledge' },
]

const DOCK_TABS: { id: DockTab; label: string }[] = [
  { id: 'code', label: 'Code intelligence' },
  { id: 'memory', label: 'Memory' },
  { id: 'knowledge', label: 'Knowledge' },
]

// provenance legend for the status bar
// Notes are terse on purpose: six chip+note pairs plus the left-hand status wrapped
// the bar onto a second row at 1280px, and the bar is inside the section's height
// budget (tests/e2e/section-fits.spec.ts).
const PROV_LEGEND: { prov: string; note: string; accent?: boolean }[] = [
  { prov: 'Parsed', note: 'AST · 1.0', accent: true },
  { prov: 'SCIP', note: 'indexer · 1.0', accent: true },
  { prov: 'Injected', note: 'ExtraEdge', accent: true },
  { prov: 'Memory', note: 'salience' },
  { prov: 'Knowledge', note: 'RRF' },
  { prov: 'Tags', note: '0.3' },
]

function confColor(conf: number) {
  if (conf >= 1.0) return 'var(--accent)'
  if (conf >= 0.8) return 'var(--ink)'
  if (conf >= 0.6) return 'var(--muted)'
  return 'var(--faint)'
}

function cmdById(id: string | undefined) { return COMMANDS.find(c => c.id === id) }

// a small pointer cursor for the auto-play gestures
function CursorArrow() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 2l15 8.5-6.4 1.4L9.6 20 4 2z" fill="var(--ink)" stroke="var(--canvas)" strokeWidth="1.4" strokeLinejoin="round" />
    </svg>
  )
}

// one provenance chip + confidence value, reused across code / memory / knowledge rows
function Chips({ prov, conf, advisory, on, injected }: { prov: string; conf: number; advisory?: boolean; on: boolean; injected?: boolean }) {
  return (
    <span className="ide-line-chips">
      <span className="prov" style={conf >= 1.0 || injected ? { color: 'var(--accent)', borderColor: 'color-mix(in oklab, var(--accent) 45%, var(--hairline))' } : undefined}>{prov}</span>
      <span className="prov tabular-nums" style={{ color: confColor(conf) }}>{conf.toFixed(2)}</span>
      {advisory && <span className="prov">adv</span>}
      {!on && <span className="prov" style={{ color: 'var(--faint)' }}>below cutoff</span>}
    </span>
  )
}

function AgentIDE() {
  const reduced = useReducedMotion()
  const isMobile = useIsMobile()
  const [driving, setDriving] = useState(false)
  const [threshold, setThreshold] = useState(0.55)
  const [step, setStep] = useState(0)
  const [inView, setInView] = useState(false)
  const [cursorAt, setCursorAt] = useState<SymId | null>(STORYBOARD[0].target)
  const [menuOpen, setMenuOpen] = useState(false)
  const [menuRun, setMenuRun] = useState<string | null>(null)
  const [codePeek, setCodePeek] = useState<Peek>(cmdById(STORYBOARD[0].run)?.peek ?? DETAILS)
  const [dockTab, setDockTab] = useState<DockTab>('code')
  const [memHighlight, setMemHighlight] = useState<string | null>(null)

  // apply a command's result: land it in the right dock tab
  const applyCommand = (id: string) => {
    const c = cmdById(id); if (!c) return
    setDockTab(c.lands)
    if (c.lands === 'code' && c.peek) { setCodePeek(c.peek); setMemHighlight(null) }
    else if (c.lands === 'memory') setMemHighlight(c.memId ?? null)
    else setMemHighlight(null)
  }

  // ── AUTO-PLAY · a storyboard of real IDE gestures (only while on screen) ─────
  useEffect(() => {
    if (driving || reduced || !inView || isMobile) return
    const g = STORYBOARD[step]
    setCursorAt(g.target); setMenuOpen(false); setMenuRun(null)
    const timers: number[] = []
    if (g.kind === 'menu') {
      timers.push(window.setTimeout(() => setMenuOpen(true), 620))
      timers.push(window.setTimeout(() => setMenuRun(g.run ?? null), 1200))
      timers.push(window.setTimeout(() => { setMenuOpen(false); setMenuRun(null); if (g.run) applyCommand(g.run) }, 1820))
    } else {
      timers.push(window.setTimeout(() => { setCodePeek(DETAILS); setDockTab('code'); setMemHighlight(null) }, 980))
    }
    timers.push(window.setTimeout(() => setStep(s => (s + 1) % STORYBOARD.length), 2650))
    return () => timers.forEach(clearTimeout)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, driving, reduced, inView, isMobile])

  // reduced motion: no cursor / menu; show a static, representative result set
  useEffect(() => {
    if (reduced) { setCursorAt(null); setMenuOpen(false); setCodePeek(DETAILS); setDockTab('code') }
  }, [reduced])

  // auto-play the moment the section scrolls into view; pause when it scrolls out.
  // Arriving shows it already cycling: entry restarts the storyboard from step 0.
  const rootRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = rootRef.current
    if (!el || typeof IntersectionObserver === 'undefined') { setInView(true); return }
    let fired = false
    const io = new IntersectionObserver(([e]) => { fired = true; setInView(e.isIntersecting) }, { threshold: 0.2 })
    io.observe(el)
    // safety net: in a few embedded/automation contexts IO never delivers — rather than
    // sit frozen, fall back to always-animating if no callback has arrived.
    const t = window.setTimeout(() => { if (!fired) setInView(true) }, 1600)
    return () => { io.disconnect(); clearTimeout(t) }
  }, [])
  useEffect(() => { if (inView && !driving) setStep(0) }, [inView, driving])

  const takeControl = () => { setDriving(true); setCursorAt(null); setMenuOpen(false); setMenuRun(null) }
  const runCommand = (id: string) => { takeControl(); applyCommand(id) }
  const openMenuAt = (sym: SymId) => { setDriving(true); setMenuRun(null); setCursorAt(sym); setMenuOpen(true) }
  const clickSymbol = (sym: SymId) => {
    if (sym === 'PricingService') { setDriving(true); setMenuOpen(false); setMenuRun(null); setCursorAt(sym); setCodePeek(DETAILS); setDockTab('code'); setMemHighlight(null) }
    else openMenuAt(sym)
  }

  // cursor + menu positioning: measure the target symbol within the code pane
  const codeRef = useRef<HTMLDivElement>(null)
  const symRefs = useRef<Record<string, HTMLElement | null>>({})
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null)
  useEffect(() => {
    const host = codeRef.current
    const el = cursorAt ? symRefs.current[cursorAt] : null
    if (!host || !el) { setPos(null); return }
    const hr = host.getBoundingClientRect(); const r = el.getBoundingClientRect()
    setPos({ x: r.left - hr.left + r.width * 0.6, y: r.top - hr.top + r.height * 0.72 })
  }, [cursorAt])

  const showCursor = inView && !driving && !reduced && !!pos
  const showMenu = menuOpen && !!pos

  // per-tab live counts (gated by the dial) for the dock tab badges
  const codeLive = codePeek.facts.filter(f => f.conf >= threshold).length
  const memLive = MEMORIES.filter(m => m.salience >= threshold).length
  const knowLive = KNOWLEDGE.filter(k => k.conf >= threshold).length
  const tabCount = (id: DockTab) => id === 'code' ? codeLive : id === 'memory' ? memLive : knowLive
  const statusTitle = dockTab === 'code' ? codePeek.title : dockTab === 'memory' ? 'Recall memory' : 'Search knowledge'
  const statusLive = dockTab === 'code' ? codeLive : dockTab === 'memory' ? memLive : knowLive
  const statusTotal = dockTab === 'code' ? codePeek.facts.length : dockTab === 'memory' ? MEMORIES.length : KNOWLEDGE.length

  // ── confidence gutter — the dial gates every dock result. Shared by the
  //    desktop split-pane and the mobile stacked treatment. ─────────────────
  const gutterEl = (
    <div className="ide-gutter">
      <span className="ide-gutter-label">confidence ≥</span>
      <span className="ide-gutter-val tabular-nums">{threshold.toFixed(2)}</span>
      <input
        type="range" min={0.3} max={1.0} step={0.05} value={threshold}
        onChange={e => { takeControl(); setThreshold(parseFloat(e.target.value)) }}
        onMouseDown={takeControl} onTouchStart={takeControl}
        className="dial ide-dial" aria-label="Confidence threshold"
      />
      <span className="ide-gutter-ends">
        <span>0.30 · heuristics</span>
        <span>1.00 · SCIP only</span>
      </span>
    </div>
  )

  // ── the DOCK — tabbed: Code intelligence · Memory · Knowledge. Shared by both
  //    layouts; on mobile the CSS lets its rows wrap instead of ellipsis-truncate. ─
  const dockEl = (
    <div className="ide-dock">
      <div className="ide-dock-tabs" role="tablist" aria-label="Estate panels">
        {DOCK_TABS.map(t => (
          <button
            key={t.id}
            id={`ide-tab-${t.id}`}
            role="tab"
            aria-selected={dockTab === t.id}
            className="ide-dock-tab"
            data-on={String(dockTab === t.id)}
            onClick={() => { takeControl(); setDockTab(t.id) }}
          >
            {t.label}
            <span className="ide-dock-badge">{tabCount(t.id)}</span>
          </button>
        ))}
      </div>

      <div className="ide-dock-body" id="ide-dock-panel" role="tabpanel" aria-labelledby={`ide-tab-${dockTab}`} key={dockTab + (dockTab === 'code' ? codePeek.title : '')}>
        {dockTab === 'code' && (
          <>
            <div className="ide-dock-head">
              <span className="ide-dock-title">{codePeek.title}</span>
              <span className="ide-dock-sub depth">{codePeek.sub}</span>
              {codePeek.wiki && <span className="ide-wiki-ref">{codePeek.wiki}</span>}
            </div>
            {codePeek.facts.map((f, i) => {
              const on = f.conf >= threshold
              const injected = f.prov === 'Injected'
              return (
                <div key={i} className="ide-line fact" data-on={String(on)}>
                  <span className="ide-ln">{i + 1}</span>
                  <span className="ide-line-body">
                    <span className="ide-line-top">
                      <span className="ide-line-text" title={f.text}>{f.text}</span>
                      <Chips prov={f.prov} conf={f.conf} advisory={f.advisory} on={on} injected={injected} />
                    </span>
                    {f.detail && <span className="ide-line-detail" data-injected={String(injected)} title={f.detail}>{f.detail}</span>}
                  </span>
                </div>
              )
            })}
          </>
        )}

        {dockTab === 'memory' && (
          <>
            <div className="ide-dock-head">
              <span className="ide-dock-title">Recalled memory</span>
              <span className="ide-dock-sub depth">decisions · patterns · gotchas — scoped · erasable by subtree</span>
            </div>
            {MEMORIES.map(m => {
              const on = m.salience >= threshold
              const hot = memHighlight === m.id
              return (
                <div key={m.id} className="ide-mem" data-on={String(on)} data-hot={String(hot)} data-superseded={String(!!m.superseded)}>
                  <span className="ide-mem-kind" data-kind={m.kind}>{m.kind}</span>
                  <span className="ide-mem-body">
                    <span className="ide-mem-top">
                      <span className="ide-mem-text" title={m.text}>{m.text}</span>
                      <span className="ide-line-chips">
                        <span className="prov" style={m.salience >= 0.85 ? { color: 'var(--accent)', borderColor: 'color-mix(in oklab, var(--accent) 45%, var(--hairline))' } : undefined}>{m.scope}</span>
                        <span className="prov tabular-nums" style={{ color: confColor(m.salience) }}>{m.salience.toFixed(2)}</span>
                        {m.superseded && <span className="prov">superseded</span>}
                        {!on && <span className="prov" style={{ color: 'var(--faint)' }}>below cutoff</span>}
                      </span>
                    </span>
                    <span className="ide-mem-meta depth">{m.prov} · recalled because you&apos;re editing <b className="ide-mem-link">{m.because}</b></span>
                  </span>
                </div>
              )
            })}
          </>
        )}

        {dockTab === 'knowledge' && (
          <>
            <div className="ide-dock-head">
              <span className="ide-dock-title">Knowledge · the wiki</span>
              <span className="ide-dock-sub depth">hybrid FTS + vector, RRF-fused · r@10 0.849 on the parity bench</span>
            </div>
            {KNOWLEDGE.map(k => {
              const on = k.conf >= threshold
              return (
                <div key={k.id} className="ide-line fact" data-on={String(on)}>
                  <span className="ide-ln ide-know-ln" aria-hidden="true">§</span>
                  <span className="ide-line-body">
                    <span className="ide-line-top">
                      <span className="ide-line-text" title={k.detail}>
                        <b className="ide-wiki-ref ide-wiki-inline">[[{k.title}]]</b> {k.section}
                      </span>
                      <Chips prov="Knowledge" conf={k.conf} on={on} />
                    </span>
                    <span className="ide-line-detail" title={k.detail}>{k.detail}</span>
                  </span>
                </div>
              )
            })}
          </>
        )}
      </div>
    </div>
  )

  // the status bar — reused verbatim across both layouts
  const statusBarEl = (
    <div className="ide-statusbar">
      <span className="ide-status-left">
        <span className="ide-status-seg" data-accent="true">{statusTitle}</span>
        <span className="ide-sep" aria-hidden="true">·</span>
        <span className="ide-status-seg">{statusLive}/{statusTotal} results</span>
        <span className="ide-sep" aria-hidden="true">·</span>
        <span className="ide-status-seg">confidence-gated</span>
      </span>
      <span className="ide-status-legend">
        <span className="ide-legend-label">provenance</span>
        {PROV_LEGEND.map(p => (
          <span key={p.prov} className="ide-legend-item">
            <span className="prov" style={p.accent ? { color: 'var(--accent)', borderColor: 'color-mix(in oklab, var(--accent) 45%, var(--hairline))' } : undefined}>{p.prov}</span>
            <span className="depth">{p.note}</span>
          </span>
        ))}
      </span>
    </div>
  )

  // ── MOBILE · a legible stacked treatment. The desktop split-pane (absolute
  //    cursor + right-click context menu + measured positioning) can't survive a
  //    phone, so on ≤760px we render a static, tappable representation that still
  //    tells the whole story: the file, the IDE gestures (find-callers / peek /
  //    recall / blast-radius) as tap targets, the confidence dial that gates, and
  //    the Code-intelligence · Memory · Knowledge dock. No auto-play, no overlays,
  //    nothing that can overflow. ────────────────────────────────────────────────
  const tapCommand = (id: string) => { setDriving(true); applyCommand(id) }
  const tapSymbol = (sym: SymId) => {
    setDriving(true)
    setCursorAt(sym)
    if (sym === 'PricingService') { setCodePeek(DETAILS); setDockTab('code'); setMemHighlight(null) }
    else applyCommand('callers')
  }
  if (isMobile) {
    return (
      <Section id="query" solid>
        <div className="max-w-6xl mx-auto w-full" ref={rootRef}>
          <div className="mb-4 w-full text-left">
            <span className="kicker">The agent&apos;s IDE</span>
            <h2 className="mt-1.5 font-display text-2xl font-black text-ink leading-[0.98]">
              The agent doesn&apos;t grep. It edits in an IDE that <span style={{ color: 'var(--accent)' }}>knows your whole system.</span>
            </h2>
            <p className="mt-1.5 text-sm text-muted font-sans leading-tight">
              Tap a symbol or an IDE gesture — every caller, the whole picture of a class, the requirement it
              satisfies, the <span className="text-ink">decision</span> behind it, the <span className="text-ink">wiki</span>,
              the blast radius. Each answer a live fact with <span className="text-ink">confidence + provenance</span>;
              drop the dial and the low-confidence guesses fall out, labeled.
            </p>
          </div>

          <div className="ide-window ide-window--mobile">
            <div className="ide-chrome">
              <span className="ide-lights" aria-hidden="true"><i /><i /><i /></span>
              <span className="ide-title">wicked-estate — the agent&apos;s workspace</span>
            </div>

            {/* the file — static, tappable symbols */}
            <div className="ide-tabbar">
              <span className="ide-tab" data-on="true">
                <span className="ide-glyph" data-lang="TS">TS</span>
                <span className="ide-tab-name">PricingService.ts</span>
              </span>
            </div>
            <div className="ide-code ide-code--mobile">
              <pre className="ide-code-pre"><code>{CODE.map((line, li) => (
                <span className="ide-code-line" key={li}>
                  <span className="ide-code-ln">{li + 1}</span>
                  <span className="ide-code-toks">
                    {line.map((tk, ti) => tk.sym ? (
                      <button
                        key={ti}
                        className="ide-sym"
                        data-k={tk.k}
                        data-active={String(cursorAt === tk.sym)}
                        onClick={() => tapSymbol(tk.sym!)}
                      >{tk.t}</button>
                    ) : (
                      <span key={ti} className="ide-tok" data-k={tk.k}>{tk.t}</span>
                    ))}
                  </span>
                </span>
              ))}</code></pre>
            </div>

            {/* IDE gestures — the command palette, as tap targets */}
            <div className="ide-m-gestures">
              <span className="ide-sec-label">IDE gestures · tap one — the record answers</span>
              <ul className="ide-m-cmds" role="list">
                {COMMANDS.map(c => {
                  const active = (c.lands === 'code' && dockTab === 'code' && codePeek.title === c.peek?.title)
                    || (c.lands === 'memory' && dockTab === 'memory')
                    || (c.lands === 'knowledge' && dockTab === 'knowledge')
                  return (
                    <li key={c.id}>
                      <button className="ide-m-cmd" data-active={String(active)} aria-pressed={active} onClick={() => tapCommand(c.id)}>
                        <span className="ide-cmd-label">{c.label}</span>
                        {c.note && <span className="ide-kbd">{c.note}</span>}
                      </button>
                    </li>
                  )
                })}
              </ul>
            </div>

            {gutterEl}
            {dockEl}
            {statusBarEl}
          </div>
        </div>
      </Section>
    )
  }

  return (
    <Section id="query" solid>
      <div className="max-w-6xl mx-auto w-full" ref={rootRef}>
        {/* section header — using an IDE that actually knows your system */}
        <div className="mb-1.5 w-full text-left">
          <span className="kicker">The agent&apos;s IDE</span>
          <h2 className="mt-1.5 font-display text-2xl sm:text-[1.7rem] font-black text-ink leading-[1.0] max-w-3xl">
            The agent doesn&apos;t grep. It edits in an IDE that <span style={{ color: 'var(--accent)' }}>knows your whole system.</span>
          </h2>
          <p className="mt-1.5 text-[0.82rem] text-muted font-sans leading-tight max-w-5xl">
            Right-click a symbol for every caller, click a class for its whole picture, trace the requirement it
            satisfies, recall the <span className="text-ink">decision</span> behind it, pull the{' '}
            <span className="text-ink">wiki</span>, see the blast radius. Every answer a live fact with{' '}
            <span className="text-ink">confidence + provenance</span> — drop the dial and the low-confidence guesses
            fall out, labeled, never silently promoted.
          </p>
        </div>

        {/* ── THE IDE WINDOW ────────────────────────────────────────────── */}
        <div className="ide-window">
          {/* chrome bar */}
          <div className="ide-chrome">
            <span className="ide-lights" aria-hidden="true"><i /><i /><i /></span>
            <span className="ide-title">wicked-estate — the agent&apos;s workspace</span>
            <button
              className="demo-pill ide-run"
              data-live={String(!driving)}
              onClick={() => setDriving(d => !d)}
              aria-label={driving ? 'Resume the auto demo' : 'Pause and drive the workspace yourself'}
            >
              <span className="dot" />
              {driving ? 'Driving · resume' : 'Auto-demo · drive'}
            </button>
          </div>

          {/* body — explorer + editor */}
          <div className="ide-body">
            {/* LEFT · explorer */}
            <aside className="ide-explorer" aria-label="Explorer">
              <div>
                <span className="ide-sec-label">Explorer · checkout/PricingService.ts</span>
                <ul className="ide-tree" role="list">
                  {([['PricingService', 'CLS', 'class'], ['applyDiscount', 'FN', 'method']] as [SymId, string, string][]).map(([sym, glyph, kind]) => (
                    <li key={sym}>
                      <button className="ide-file" data-on={String(cursorAt === sym)} onClick={() => clickSymbol(sym)}>
                        <span className="ide-glyph" data-lang={glyph}>{glyph}</span>
                        <span className="ide-file-name">{sym}</span>
                        <span className="ide-file-kind">{kind}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </div>

              <div>
                <span className="ide-sec-label">IDE commands · right-click a symbol</span>
                <ul className="ide-actions" role="list">
                  {COMMANDS.map(c => {
                    const active = (c.lands === 'code' && dockTab === 'code' && codePeek.title === c.peek?.title)
                      || (c.lands === 'memory' && dockTab === 'memory')
                      || (c.lands === 'knowledge' && dockTab === 'knowledge')
                    return (
                      <li key={c.id}>
                        <button className="ide-action ide-cmd" data-active={String(active)} aria-pressed={active} onClick={() => runCommand(c.id)}>
                          <span className="ide-cmd-label">{c.label}</span>
                          {c.note && <span className="ide-kbd">{c.note}</span>}
                        </button>
                      </li>
                    )
                  })}
                </ul>
              </div>
            </aside>

            {/* CENTER · editor */}
            <div className="ide-editor">
              {/* tab bar */}
              <div className="ide-tabbar">
                <span className="ide-tab" data-on="true">
                  <span className="ide-glyph" data-lang="TS">TS</span>
                  <span className="ide-tab-name">PricingService.ts</span>
                </span>
                <span className="ide-crumb depth">checkout/ · right-click a symbol → the record answers</span>
              </div>

              {/* the code — clickable symbols, with the cursor + context menu overlay */}
              <div className="ide-code" ref={codeRef}>
                <pre className="ide-code-pre"><code>
                  {CODE.map((line, li) => (
                    <span className="ide-code-line" key={li}>
                      <span className="ide-code-ln">{li + 1}</span>
                      <span className="ide-code-toks">
                        {line.map((tk, ti) => tk.sym ? (
                          <button
                            key={ti}
                            ref={el => { symRefs.current[tk.sym!] = el }}
                            className="ide-sym"
                            data-k={tk.k}
                            data-active={String(cursorAt === tk.sym)}
                            onClick={() => clickSymbol(tk.sym!)}
                            onContextMenu={e => { e.preventDefault(); openMenuAt(tk.sym!) }}
                          >{tk.t}</button>
                        ) : (
                          <span key={ti} className="ide-tok" data-k={tk.k}>{tk.t}</span>
                        ))}
                      </span>
                    </span>
                  ))}
                </code></pre>

                {showMenu && (
                  <button className="ide-ctx-backdrop" aria-label="Close menu" onClick={() => setMenuOpen(false)} />
                )}
                {showCursor && (
                  <span className="ide-cursor" style={{ left: pos!.x, top: pos!.y }} aria-hidden="true"><CursorArrow /></span>
                )}
                {showMenu && (
                  <div className="ide-ctxmenu" style={{ left: pos!.x, top: pos!.y }} role="group" aria-label="Estate commands">
                    <div className="ide-ctxmenu-head">estate</div>
                    {COMMANDS.map(c => (
                      <button key={c.id} className="ide-ctxmenu-item" data-run={String(menuRun === c.id)} onClick={() => runCommand(c.id)}>
                        <span className="ide-ctxmenu-label">{c.label}</span>
                        {c.note && <span className="ide-ctxmenu-note">{c.note}</span>}
                      </button>
                    ))}
                  </div>
                )}
              </div>

              {gutterEl}
              {dockEl}
            </div>
          </div>

          {statusBarEl}
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
          title={<>Five bands. One record. <span style={{ color: 'var(--accent)' }}>One symbol identity.</span></>}
        >
          <p className="max-w-4xl">
            Not a pile of scanners — a single continuous body, cut into bands. Each keyed to the same stable{' '}
            <span className="font-mono text-sm font-semibold">(scheme, qualified-name)</span> that survives reformatting,
            moves, and re-index.
          </p>
        </TopHead>

        <div className="mb-2">
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
                  className="relative flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-5 px-5 py-2.5 border-b border-hairline last:border-b-0 transition-all duration-300 cursor-pointer"
                  style={{
                    background: on
                      ? 'color-mix(in oklab, var(--accent) 9%, transparent)'
                      : (i % 2 ? 'color-mix(in oklab, var(--ink) 3%, transparent)' : 'transparent'),
                    boxShadow: on ? 'inset 3px 0 0 var(--accent)' : 'none',
                  }}
                >
                  <div className="flex items-center gap-3 sm:w-60 shrink-0">
                    <span className="font-display font-black text-xl tabular-nums" style={{ fontStretch: '108%', color: on ? 'var(--accent)' : 'var(--faint)' }}>{s.no}</span>
                    <div>
                      <div className="font-display font-black text-ink text-base leading-tight" style={{ fontStretch: '108%' }}>{s.name}</div>
                      <div className="depth">{s.depth} · {s.tools}</div>
                    </div>
                  </div>
                  <p className="text-[0.82rem] font-sans flex-1 leading-[1.45] transition-colors" style={{ color: on ? 'var(--ink)' : 'var(--muted)' }}>{s.copy}</p>
                </div>
              )
            })}
          </div>
        </div>
        <p className="mt-3 font-mono text-[0.68rem] text-faint">
          Code graph · injected edges · memory · knowledge · provenance — one binary, every fact stamped with confidence and provenance.
        </p>
      </div>
    </Section>
  )
}

// ── 3b · THE FULL TOOLFACE — every tool + skill an agent can call ───────────────
// Grounded to v0.14.4. Tools: crates/wicked-estate-mcp/src/lib.rs (dispatch +
// memory/knowledge schemas), README.md §MCP. Skills: crates/*/skills/*/SKILL.md.
type ToolDomain = { no: string; name: string; note: string; tools: { name: string; purpose: string }[] }

const TOOL_DOMAINS: ToolDomain[] = [
  {
    no: '01', name: 'Graph', note: '10 estate tools',
    tools: [
      { name: 'SearchEntity',   purpose: 'Find symbols by name or kind; optional source inline.' },
      { name: 'RetrieveEntity', purpose: 'Full dossier: callers, edges, annotations, requirement.' },
      { name: 'TraverseGraph',  purpose: 'Bounded walk out over calls and imports.' },
      { name: 'BlastRadius',    purpose: 'Every dependent — what breaks if you change it.' },
      { name: 'Lineage',        purpose: 'The dependency chain a symbol rests on, transitively.' },
      { name: 'RankHotspots',   purpose: 'Most-connected symbols by PageRank — where to start.' },
      { name: 'Communities',    purpose: 'Clusters the graph into modules.' },
      { name: 'ContextBundle',  purpose: 'Scoped, prompt-ready context pack for a symbol.' },
      { name: 'FetchContent',   purpose: 'The stored source for a symbol or file.' },
      { name: 'RulesInventory', purpose: 'ODM · DMN · Drools · CLIPS engines + their callers.' },
    ],
  },
  {
    no: '02', name: 'Memory', note: '6 memory tools',
    tools: [
      { name: 'memory.capture',  purpose: 'Capture a node — episodic, semantic, procedural, archival.' },
      { name: 'memory.recall',   purpose: 'Token-budgeted recall relevant to a query in scope.' },
      { name: 'memory.learn',    purpose: 'Store a semantic fact and link it to code symbols atomically.' },
      { name: 'memory.reflect',  purpose: 'Distil episodic memories in a scope into semantic facts.' },
      { name: 'memory.coverage', purpose: 'Node counts by tier and kind.' },
      { name: 'memory.erase',    purpose: 'Hard-delete everything under a scope prefix.' },
    ],
  },
  {
    no: '03', name: 'Knowledge', note: '7 knowledge tools',
    tools: [
      { name: 'knowledge.ingest',            purpose: 'Ingest a document as a doc + retrievable chunk nodes.' },
      { name: 'knowledge.write',             purpose: 'Write one node (doc / section / chunk / concept).' },
      { name: 'knowledge.relate',            purpose: 'Add a typed, confidence-scored relation between nodes.' },
      { name: 'knowledge.recall',            purpose: 'Hybrid FTS + vector recall, RRF-fused.' },
      { name: 'knowledge.relate_code',       purpose: 'Link a knowledge node to code symbols in the graph.' },
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
  { name: 'ontology-expedition',  purpose: 'Connect concepts with typed relations — structure over a flat store.' },
  { name: 'knowledge-curation',   purpose: 'Dedup as the base grows — collapse-but-surface, never delete.' },
  { name: 'gap-hunting',          purpose: 'Turn recall misses into ingest tasks — close the loop.' },
]

// Cross-cutting capabilities, each grounded in a repo path.
const CAPABILITIES: string[] = [
  'Injected edges · ExtraEdge TOML rules · event→consumer · command→agent',
  'Every edge: confidence + provenance + resolved_by',
  '7 resolution tiers · Parsed → SCIP → LSP',
  'Rules engines in the same graph · ODM · DMN · Drools',
  'Requirement ↔ code traceability',
  'Typed annotations · survive re-index',
  'Memory scopes · subtree recall + erase',
  'SQLite by default · Postgres for teams',
  '102 wired languages — a manifest row + a query file',
  '+ SemanticSearch, an 11th graph tool, when an embedder is wired',
]

// The retrieval receipt — the S3 parity bench that gated the wicked-brain
// consolidation: 60 queries × 5 classes over an identical 357-doc corpus,
// estate's knowledge surface vs the dedicated FTS search it replaced.
const BENCH = {
  rows: [
    { metric: 'overall r@10', estate: '0.849', fts: '0.437' },
    { metric: 'overall MRR',  estate: '0.810', fts: '0.571' },
  ],
  note: 'Ahead on four of five query classes; within 2.2% on the fifth — identifier-shaped queries, the FTS system’s home turf.',
}

function FullToolface() {
  return (
    <Section id="toolface" solid>
      {/* Wider measure than its neighbours (max-w-6xl) on purpose: this section's cost is
          23 tool rows whose purpose column was squeezed to ~250px, so nearly every one
          wrapped to two lines and the ten-row Graph panel paid for it ten times. The extra
          ~45px of measure buys back a line on several rows — cheaper than cutting copy. */}
      <div className="max-w-7xl mx-auto w-full">
        <div className="mb-2 w-full text-left">
          <span className="kicker">Everything an agent can call</span>
          <h2 className="mt-1.5 font-display text-2xl sm:text-[1.8rem] font-black text-ink leading-[1.0]">
            23 MCP tools. 6 agent skills. <span style={{ color: 'var(--accent)' }}>One binary.</span>
          </h2>
          <p className="mt-1.5 text-[0.82rem] text-muted font-sans leading-tight max-w-5xl">
            Not one “search” tool bolted onto a repo — the full MCP surface an agent can call, plus the skills
            (playbooks) that drive them. Memory and knowledge are where the wicked-brain consolidation landed:
            one store, one identity, a <span className="text-ink">lossless import contract</span> so nothing tuned
            was dropped. Grounded to v0.14.4.
          </p>
        </div>

        {/* The three MCP domains — every tool name + one-line purpose.
            NOT three equal columns. The grid is as tall as its TALLEST panel, and the
            panels hold 10 / 6 / 7 rows, so width spent on Memory is width that can never
            shorten the section while width spent on Graph can: a wider purpose column
            un-wraps its rows, and Graph pays for a wrapped row ten times. Memory has four
            rows of headroom to give away. */}
        <div className="grid lg:grid-cols-[1.20fr_0.74fr_1.06fr] gap-2">
          {TOOL_DOMAINS.map(d => (
            <div key={d.name} className="rock-panel p-0">
              <div className="flex items-center gap-2.5 px-4 py-1 border-b border-hairline-strong">
                <span className="font-display font-black text-[0.95rem] tabular-nums" style={{ fontStretch: '108%', color: 'var(--accent)' }}>{d.no}</span>
                <span className="font-display font-black text-ink text-[0.9rem] flex-1" style={{ fontStretch: '108%' }}>{d.name}</span>
                <span className="tag tag-accent">{d.note}</span>
              </div>
              <div className="divide-y divide-hairline">
                {d.tools.map(t => (
                  <div key={t.name} className="tool-row">
                    <span className="tool-row-name">{t.name}</span>
                    <span className="tool-row-purpose">{t.purpose}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* the retrieval receipt — the bench that gated the consolidation. One row, not
            two: the label, the corpus and both metrics read as a single receipt line. */}
        <div className="mt-1 rock-panel p-0" id="bench">
          <div className="px-4 py-1.5 flex flex-wrap items-baseline gap-x-5 gap-y-1">
            <span className="kicker" style={{ color: 'var(--accent)' }}>The retrieval receipt</span>
            {BENCH.rows.map(r => (
              <span key={r.metric} className="inline-flex items-baseline gap-2">
                <span className="depth">{r.metric}</span>
                <span className="font-mono text-[0.82rem] font-bold" style={{ color: 'var(--accent)' }}>{r.estate}</span>
                <span className="depth">vs</span>
                <span className="font-mono text-[0.82rem] text-muted">{r.fts}</span>
              </span>
            ))}
            <span className="depth flex-1 min-w-[20rem]" style={{ whiteSpace: 'normal' }}>
              60 queries × 5 classes, identical 357-doc corpus, vs the dedicated FTS search it replaced. {BENCH.note}
            </span>
          </div>
        </div>

        {/* the agent skills — the playbooks that ship with estate */}
        <div className="mt-1 rock-panel p-0">
          <div className="flex items-center gap-3 px-4 py-1.5 border-b border-hairline-strong">
            {/* no "6 skills" chip — the section headline already says "6 agent skills",
                and the chip's own height set this header's height */}
            <span className="kicker">Agent skills · in-repo playbooks</span>
          </div>
          <div className="grid sm:grid-cols-2 lg:grid-cols-3">
            {AGENT_SKILLS.map(s => (
              <div key={s.name} className="skill-row">
                <span className="tool-row-name">{s.name}</span>
                <span className="tool-row-purpose">{s.purpose}</span>
              </div>
            ))}
          </div>
        </div>

        {/* cross-cutting capabilities most agents never know the record has */}
        <div className="mt-1 flex flex-wrap gap-1">
          {CAPABILITIES.map(c => <span key={c} className="tag tag-tight">{c}</span>)}
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

// ── 5 · SOLO OR SHARED — same record, one profile switch ────────────────────────
function Storage() {
  const [shared, setShared] = useState(false)
  const view = shared
    ? {
        cmd: 'WICKED_RUNTIME=team wicked-estate index .',
        rows: [
          ['store', 'WICKED_STORE_URL=postgres://team/graph — shared, self-hosted'],
          ['writers', 'concurrent — the whole team writes'],
          ['traversal', 'server-side WITH RECURSIVE, in-DB'],
          ['switching', 'one profile switch — same schema, no re-index'],
        ],
        note: '--features postgres · fail-loud: a typo’d profile errors, never silently local',
      }
    : {
        cmd: 'wicked-estate index . --db graph.db',
        rows: [
          ['store', 'graph.db — one local file, repo-scoped'],
          ['writers', 'single-writer — no daemon, no races'],
          ['traversal', 'bounded recursive CTE'],
          ['infrastructure', 'none — nothing to run, nothing leaves your box'],
        ],
        note: 'FTS5 · sqlite-vec · WAL · the zero-infra default',
      }
  return (
    <Section id="storage" solid>
      <div className="max-w-4xl mx-auto w-full">
        <TopHead
          kicker="One record, solo or shared"
          title={<>SQLite by default. <span style={{ color: 'var(--accent)' }}>One profile switch</span> to a shared team backend.</>}
        >
          <p className="max-w-4xl">
            The same engine and the same 23 tools run on either backend through one{' '}
            <span className="font-mono text-sm font-semibold">open_store(spec)</span> factory — no caller changes,
            no re-index. The Postgres backend passes the same store-conformance suite in CI, and the{' '}
            <span className="font-mono text-sm font-semibold">WICKED_RUNTIME=team</span> profile seam retargets the
            whole foundation with one environment variable. Local-first is a feature, not a ceiling.
          </p>
        </TopHead>

        <div className="inline-flex flex-wrap gap-1.5 p-1 rounded-xl mb-4" style={{ background: 'var(--rock)', border: '1px solid var(--hairline-strong)' }}>
          <button className="seg" data-on={!shared} onClick={() => setShared(false)}>SQLite · solo</button>
          <button className="seg" data-on={shared} onClick={() => setShared(true)}>PostgreSQL · shared team</button>
        </div>

        <div className="rock-panel p-0">
          <div className="px-5 py-2.5 border-b border-hairline-strong flex items-center gap-3">
            <span className="w-2 h-2 rounded-full" style={{ background: shared ? 'var(--accent)' : 'var(--muted)' }} />
            <span className="font-mono text-sm text-ink">{view.cmd}</span>
          </div>
          <div className="divide-y divide-hairline">
            {view.rows.map(([k, v]) => (
              <div key={k} className="flex gap-4 px-5 py-2.5">
                <span className="font-mono text-xs text-muted w-40 shrink-0">{k}</span>
                <span className="text-[0.82rem] text-ink font-sans">{v}</span>
              </div>
            ))}
          </div>
          <div className="px-5 py-2.5 border-t border-hairline-strong">
            <span className="depth" style={{ color: shared ? 'var(--accent)' : 'var(--faint)' }}>{view.note}</span>
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 6 · GET STARTED (exported separately — the SameGarden four-plane map, an
//        Astro component, renders between the main island and this one) ─────────
function CopyBtn({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | null>(null)
  useEffect(() => () => { if (timer.current) window.clearTimeout(timer.current) }, [])
  const copy = async () => {
    let ok = false
    try {
      if (navigator.clipboard?.writeText) { await navigator.clipboard.writeText(text); ok = true }
    } catch { /* fall through to the legacy path */ }
    if (!ok) {
      try {
        const ta = document.createElement('textarea')
        ta.value = text
        ta.setAttribute('readonly', '')
        ta.style.position = 'absolute'
        ta.style.left = '-9999px'
        document.body.appendChild(ta)
        ta.select()
        ok = document.execCommand('copy')
        ta.remove()
      } catch { ok = false }
    }
    if (ok) {
      setCopied(true)
      if (timer.current) window.clearTimeout(timer.current)
      timer.current = window.setTimeout(() => setCopied(false), 2000)
    }
  }
  return (
    <button type="button" className="copy-btn" data-copied={String(copied)} onClick={copy} aria-label={`Copy: ${label}`}>
      {copied ? 'copied ✓' : 'copy'}
    </button>
  )
}

const INSTALL_CMD = 'npx wicked-installer'
const DIRECT_CMDS: { comment: string; cmd: string; show?: string }[] = [
  { comment: '# the CLI + the MCP server', cmd: 'cargo install wicked-estate wicked-estate-mcp' },
  { comment: '# index your repo', cmd: 'wicked-estate index . --db graph.db' },
  { comment: '# connect your agent (Claude Code shown)',
    cmd: 'claude mcp add wicked-estate -s project -- wicked-estate-mcp --db "$PWD/graph.db"',
    show: 'claude mcp add wicked-estate -s project \\\n    -- wicked-estate-mcp --db "$PWD/graph.db"' },
]

export function GetStarted() {
  return (
    <Section id="get-started" solid>
      <div className="max-w-4xl mx-auto w-full">
        <TopHead
          kicker="Get started"
          title="Zero to a queried record in two minutes."
        />

        <div className="grid md:grid-cols-2 gap-5">
          {/* PRIMARY — installer */}
          <div className="rock-panel p-0">
            <div className="px-5 py-3 border-b border-hairline-strong flex items-center justify-between">
              <span className="kicker" style={{ color: 'var(--accent)' }}>Recommended · the whole family</span>
              <span className="depth">npm</span>
            </div>
            <div className="px-5 py-5">
              <div className="flex items-center gap-3">
                <div className="font-mono text-sm text-ink flex-1 min-w-0">
                  <span style={{ color: 'var(--accent)' }}>$ </span>{INSTALL_CMD}
                </div>
                <CopyBtn text={INSTALL_CMD} label={INSTALL_CMD} />
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
              {DIRECT_CMDS.map(c => (
                <div key={c.cmd} className="mb-2 last:mb-0">
                  <div className="text-faint">{c.comment}</div>
                  <div className="flex items-start gap-3">
                    <div className="text-ink flex-1 min-w-0 whitespace-pre-wrap">
                      <span style={{ color: 'var(--accent)' }}>$ </span>{c.show ?? c.cmd}
                    </div>
                    <CopyBtn text={c.cmd} label={c.cmd} />
                  </div>
                </div>
              ))}
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
// The SameGarden four-plane map (shared wicked-web chrome, an Astro component)
// renders between this island and the GetStarted island — see index.astro.
export default function Content() {
  return (
    <>
      <Hero />
      <AgentIDE />
      <FiveStrata />
      <FullToolface />
      <ProvenanceSeam />
      <Storage />
    </>
  )
}
