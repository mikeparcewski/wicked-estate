import { useState, useEffect, useRef, useCallback } from 'react'
import type { CSSProperties, ReactNode, KeyboardEvent as ReactKeyboardEvent } from 'react'

/* ────────────────────────────────────────────────────────────────────────────
   wicked-estate (we.wickedagile.com): what your agents know about your system:
   the code graph, memory and knowledge, structured and unstructured.

   Shape: the page follows ONE symbol (applyDiscount, and its event order.paid)
   through the record, one scene per frame:
     hero (what it is + a working query playground) → three stores, one record
     (the benefits on the cards) → edges grep never sees, and how sure each is →
     everything an agent can call → local by default → (SameGarden, index.astro)
     → get started.

   ACCURACY (estate 0.31.0), each grounded in the repo:
   - MCP tools: 12 graph tools always listed + SemanticSearch when embeddings are
     wired; 7 memory + 4 proposal tools; 7 knowledge tools = 30
     (crates/wicked-estate-mcp/src/lib.rs, src/tools/*.rs).
   - Tier confidences: ResolutionTier::default_confidence,
     crates/wicked-estate-core/src/edge.rs.
   - Injected edges: ExtraEdgeExtractor, plain-TOML rules, tier Heuristic (0.5),
     provenance = the rule name (crates/wicked-estate-extract/src/extra_edge.rs).
   - Memory kinds working|episode|entity|fact|skill|archive, scoped, erasable by
     subtree; operator memories apply across projects (0.30.0).
   - Two executables, the CLI (wicked-estate) and the MCP server
     (wicked-estate-mcp): the [[bin]] entries in crates/wicked-estate/Cargo.toml
     and crates/wicked-estate-mcp/Cargo.toml, shipped by
     .github/workflows/binaries.yml. Never "one binary".
   - Storage: the MCP server, memory and knowledge use local SQLite; a shared
     PostgreSQL code graph is an optional CLI build (--features postgres), and
     the MCP server refuses team mode (docs/team-runtime.md).
   - The playground is scripted: it shows the shape of real answers, it never
     runs a query. Every datum below (BLAST, WHY, STACK, the order.paid edges,
     TIERS, TOOLSETS) is that scripted data; the scenes only re-show it.

   MOTION: every animation is CSS (transform + opacity only), gated on
   html.motion-ok (set before first paint in index.astro only when the visitor
   allows motion) and on the scene's one-shot data-inview mark (src/scripts/scenes.ts).
   No loops. Pointer or key input ends the hero's intro. Reduced motion = the end state.
   ──────────────────────────────────────────────────────────────────────────── */

// The hook. Two strings, so the operator can swap a variant in one place.
const HEADLINE = 'What your agents know about your system.'
const FOLLOW = 'applyDiscount'

const vars = (o: Record<string, string | number>) => o as CSSProperties

function motionAllowed() {
  return typeof document !== 'undefined' && document.documentElement.classList.contains('motion-ok')
}

/** Calls `onEnter` once, the first time any part of `ref` rises above the lower third of the viewport.
    A line, not a visible share: a share can be unreachable (an element taller than the viewport, a short
    or zoomed window), and an entrance that never fires would leave its pre-state on screen. */
function useEnterOnce(ref: React.RefObject<HTMLElement | null>, onEnter: () => void) {
  const cb = useRef(onEnter)
  cb.current = onEnter
  useEffect(() => {
    const el = ref.current
    if (!el || typeof IntersectionObserver === 'undefined') return
    const io = new IntersectionObserver(es => {
      if (es.some(e => e.isIntersecting)) { io.disconnect(); cb.current() }
    }, { threshold: 0, rootMargin: '0px 0px -33% 0px' })
    io.observe(el)
    return () => io.disconnect()
  }, [ref])
}

/** Is `el` on screen right now (any part)? */
function onScreen(el: HTMLElement | null) {
  if (!el) return false
  const r = el.getBoundingClientRect()
  return r.bottom > 0 && r.top < window.innerHeight
}

/** Arrow keys move along a row of tabs/options (roving focus, selection follows focus). */
function arrowNav(e: ReactKeyboardEvent<HTMLElement>, selector: string, vertical = false) {
  const keys = vertical ? ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'] : ['ArrowLeft', 'ArrowRight']
  if (!keys.includes(e.key) && e.key !== 'Home' && e.key !== 'End') return
  const items = Array.from(e.currentTarget.querySelectorAll<HTMLElement>(selector))
  const at = items.indexOf(document.activeElement as HTMLElement)
  if (at < 0) return
  e.preventDefault()
  const back = e.key === 'ArrowLeft' || e.key === 'ArrowUp'
  const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : (at + (back ? -1 : 1) + items.length) % items.length
  items[next].focus()
  items[next].click()
}

function GitHubIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.02-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 012-.27c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  )
}

function Section({ id, solid = false, hero = false, children }: { id?: string; solid?: boolean; hero?: boolean; children: ReactNode }) {
  return (
    <section id={id} data-scene className={`strata${solid ? ' strata-solid' : ''}${hero ? ' strata--hero' : ''} px-4 sm:px-7`}>
      <div className="scene-in mx-auto w-full">{children}</div>
    </section>
  )
}

function Follow() {
  return <span className="follow"><i aria-hidden />following <code>{FOLLOW}</code></span>
}

function Head({ n, label, title, sub, follow = false, wide = false }: { n: string; label: string; title: ReactNode; sub?: ReactNode; follow?: boolean; wide?: boolean }) {
  return (
    <div className="scene-head">
      <div className="scene-k">
        <span className="kicker">{n} · {label}</span>
        {follow && <Follow />}
      </div>
      <h2 className="scene-h">{title}</h2>
      {sub && <p className={`scene-sub${wide ? ' scene-sub--wide' : ''}`}>{sub}</p>}
    </div>
  )
}

// ── Scripted data (the playground's record) ─────────────────────────────────
type Store = 'graph' | 'memory' | 'knowledge'
interface GEdge { from: string; to: string; via: string; tier: string; conf: number; where: string; injected?: boolean }
interface Mem { text: string; kind: string; how: 'told' | 'witnessed'; scope: string; when: string; about?: string }
interface Know { text: string; source: string; linked?: string }
interface Question { id: string; ask: string; calls: string[]; first: Store; center: string; graph: GEdge[]; memory: Mem[]; knowledge: Know[]; grep?: string }

const BLAST: GEdge[] = [
  { from: 'checkout()', to: 'applyDiscount', via: 'calls', tier: 'Parsed', conf: 1.0, where: 'src/checkout/checkout.ts:41' },
  { from: 'POST /price', to: 'applyDiscount', via: 'calls', tier: 'SCIP', conf: 1.0, where: 'src/routes/price.ts:18' },
  { from: 'sendReceipt()', to: 'applyDiscount', via: 'consumes order.paid', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true },
  { from: 'referralFlow()', to: 'applyDiscount', via: 'calls?', tier: 'Tags', conf: 0.3, where: 'tag-scan guess, unverified' },
]
const WHY: Mem[] = [
  { text: 'A discount never takes a total below zero. Two stacked coupons produced a negative total in March and refunded the customer.', kind: 'fact', how: 'told', scope: 'project:shop', when: '2026-03-14', about: 'applyDiscount' },
  { text: 'Run r-412 added the clamp and a test for it after the incident.', kind: 'episode', how: 'witnessed', scope: 'project:shop', when: '2026-03-15', about: 'applyDiscount' },
]
const STACK: Know[] = [
  { text: 'Only one percentage coupon applies per cart. Fixed-amount coupons stack, up to the cart total.', source: 'docs/pricing.md › Coupons › Stacking', linked: 'applyDiscount' },
  { text: 'Coupon rules live in one function so the price route and checkout cannot disagree.', source: 'docs/adr/ADR-014-discounts.md › Decision', linked: 'applyDiscount' },
]
const PAID: GEdge[] = [
  { from: 'checkout()', to: 'order.paid', via: 'emits', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-emit', injected: true },
  { from: 'sendReceipt()', to: 'order.paid', via: 'consumes', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true },
  { from: 'updateLedger()', to: 'order.paid', via: 'consumes', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true },
]

const QUESTIONS: Question[] = [
  { id: 'blast', ask: 'What breaks if I change applyDiscount?', calls: ['BlastRadius { symbol: "applyDiscount" }'], first: 'graph', center: 'applyDiscount',
    graph: BLAST, memory: [], knowledge: [], grep: 'grep finds 2 call sites. The graph finds a third through an event.' },
  { id: 'event', ask: 'Who reacts when an order is paid?', calls: ['TraverseGraph { symbol: "topic:order.paid" }'], first: 'graph', center: 'order.paid',
    graph: PAID, memory: [], knowledge: [], grep: 'No function calls another here. A TOML rule adds these edges at index time.' },
  { id: 'why', ask: 'Why can a discount never go below zero?', calls: ['memory.recall { query: "discount below zero", scope: "project:shop" }'], first: 'memory', center: 'applyDiscount',
    graph: [], memory: WHY, knowledge: [] },
  { id: 'stack', ask: 'Can two coupons stack?', calls: ['knowledge.recall { query: "coupon stacking" }'], first: 'knowledge', center: 'applyDiscount',
    graph: [], memory: [], knowledge: STACK },
  { id: 'all', ask: 'Everything about applyDiscount', calls: ['BlastRadius', 'memory.recall', 'knowledge.recall_about_code'], first: 'graph', center: 'applyDiscount',
    graph: BLAST, memory: WHY, knowledge: STACK },
]

const STORE_LABEL: Record<Store, { name: string; kind: string }> = {
  graph: { name: 'Code graph', kind: 'structured' },
  memory: { name: 'Memory', kind: 'told or witnessed' },
  knowledge: { name: 'Knowledge', kind: 'read from your docs' },
}

/** The demo's confidence filter: one value, shared by the hero and the confidence scene. */
const STOPS = [1.0, 0.5, 0.3] as const
type Stop = (typeof STOPS)[number]

function confClass(c: number) { return c >= 0.8 ? 'q-conf q-conf--hi' : c >= 0.5 ? 'q-conf q-conf--mid' : 'q-conf q-conf--lo' }

type Anim = 'intro' | 'switch' | 'done'

// ── 1 · Hook + the query playground ─────────────────────────────────────────
function GraphView({ q, threshold }: { q: Question; threshold: Stop }) {
  // Confidence order (1.0 first) drives the draw order and the row order.
  const ordered = [...q.graph].sort((a, b) => b.conf - a.conf)
  const rank = new Map(ordered.map((e, i) => [e.from, i]))
  const rows = ordered.filter(e => e.conf >= threshold)
  const held = q.graph.length - rows.length
  const W = 360, H = 150, cx = 180, cy = 75
  const pts = q.graph.map((_, i) => {
    const a = (Math.PI * 2 * i) / q.graph.length - Math.PI / 2
    return { x: cx + Math.cos(a) * 124, y: cy + Math.sin(a) * 54 }
  })
  return (
    <div className="q-graphview">
      {/* the proof line leads the answer: first in reading order, above the picture it describes */}
      <div className="q-figure">
        {q.grep && <p className="q-note">{q.grep}</p>}
        <svg className="q-graph" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${q.center} and the ${rows.length} edges shown at ${threshold.toFixed(1)} or above`}>
          {q.graph.map((e, i) => {
            const on = e.conf >= threshold
            const r = rank.get(e.from) ?? i
            return (
              <g key={e.from} className="q-eg" data-on={on} style={vars({ '--i': r })}>
                <line x1={cx} y1={cy} x2={pts[i].x} y2={pts[i].y} className={e.injected ? 'q-edge q-edge--inj' : e.conf < 0.5 ? 'q-edge q-edge--lo' : 'q-edge'} />
                <g className="q-en">
                  <rect x={pts[i].x - 52} y={pts[i].y - 11} width="104" height="22" rx="6" className={e.injected ? 'q-node q-node--inj' : 'q-node'} />
                  <text x={pts[i].x} y={pts[i].y + 4} textAnchor="middle" className="q-node-t">{e.from}</text>
                </g>
                {e.injected && <circle className="q-dot" cx={cx} cy={cy} r="3.5" style={vars({ '--dx': `${pts[i].x - cx}px`, '--dy': `${pts[i].y - cy}px` })} />}
              </g>
            )
          })}
          <rect x={cx - 54} y={cy - 12} width="108" height="24" rx="7" className="q-node q-node--center" />
          <text x={cx} y={cy + 4} textAnchor="middle" className="q-node-t q-node-t--center">{q.center}</text>
        </svg>
      </div>
        <div className="q-rowwrap">
          <ul className="q-rows" aria-label="Edges">
            {rows.map(e => (
              <li key={e.from} className="q-row" data-inj={e.injected ? 'true' : undefined} style={vars({ '--i': rank.get(e.from) ?? 0 })}>
                <span className="q-row-main"><b>{e.from}</b> <span className="q-via">{e.via}</span> {e.to}</span>
                <span className="q-where">{e.where}</span>
                <span className="q-chips">
                  {e.injected && <span className="q-chip q-chip--inj">injected</span>}
                  <span className="q-chip">{e.tier}</span>
                  <span className={confClass(e.conf)}>{e.conf.toFixed(1)}</span>
                </span>
              </li>
            ))}
          </ul>
          {held > 0 && (
            <p className="q-hidden" data-hidden={held}>
              {held} {held === 1 ? (threshold === 0.5 ? 'guess' : 'edge') : 'edges'} below {threshold.toFixed(1)} held back. Lower the confidence to see {held === 1 ? 'it' : 'them'}, labelled.
            </p>
          )}
        </div>
    </div>
  )
}

function MemoryView({ q }: { q: Question }) {
  return (
    <ul className="q-cards">
      {q.memory.map((m, i) => (
        <li key={m.text} className="q-card q-card--mem" style={vars({ '--i': i })}>
          <p>{m.text}</p>
          <span className="q-chips">
            <span className="q-chip q-chip--mem">{m.how}</span>
            <span className="q-chip">kind: {m.kind}</span>
            <span className="q-chip">{m.scope}</span>
            {m.about && <span className="q-chip">about: {m.about}</span>}
            <span className="q-where">{m.when}</span>
          </span>
        </li>
      ))}
    </ul>
  )
}

function KnowledgeView({ q }: { q: Question }) {
  return (
    <ul className="q-cards">
      {q.knowledge.map((k, i) => (
        <li key={k.source} className="q-card q-card--know" style={vars({ '--i': i })}>
          <p>{k.text}</p>
          <span className="q-chips">
            <span className="q-chip q-chip--know">cited</span>
            <span className="q-where">{k.source}</span>
            {k.linked && <span className="q-chip">linked to {k.linked}</span>}
          </span>
        </li>
      ))}
    </ul>
  )
}

function ConfStops({ value, onChange, id }: { value: Stop; onChange: (s: Stop) => void; id: string }) {
  return (
    <div className="q-stops" role="radiogroup" aria-labelledby={`${id}-l`} onKeyDown={e => arrowNav(e, '.q-stop', true)}>
      <span className="q-stops-l" id={`${id}-l`}>Show edges ≥</span>
      {STOPS.map(s => (
        <button key={s} type="button" role="radio" aria-checked={value === s} tabIndex={value === s ? 0 : -1} data-on={value === s} className="q-stop" onClick={() => onChange(s)}>
          {s.toFixed(1)}
        </button>
      ))}
    </div>
  )
}

interface PlayState { qid: string; tab: Store; anim: Anim; nonce: number }

function Playground({ st, pick, setTab, threshold, setThreshold }: {
  st: PlayState; pick: (id: string, tab?: Store) => void; setTab: (s: Store) => void; threshold: Stop; setThreshold: (s: Stop) => void
}) {
  const q = QUESTIONS.find(x => x.id === st.qid)!
  const counts: Record<Store, number> = { graph: q.graph.length, memory: q.memory.length, knowledge: q.knowledge.length }
  const tab = st.tab
  return (
    <div className="q-window" id="query" data-q={st.qid} data-anim={st.anim}>
      <div className="q-bar">
        <span className="q-dots" aria-hidden><i /><i /><i /></span>
        <span className="q-bar-t">wicked-estate-mcp · graph.db</span>
        <span className="q-bar-r"><Follow /><span className="q-scripted">scripted demo · pick a question</span></span>
      </div>
      <div className="q-body">
        <div className="q-asks" role="listbox" aria-label="Ask the record" onKeyDown={e => arrowNav(e, '.q-ask', true)}>
          <span className="q-k" aria-hidden>Ask</span>
          {QUESTIONS.map(x => (
            <button key={x.id} type="button" role="option" aria-selected={x.id === st.qid} tabIndex={x.id === st.qid ? 0 : -1} className="q-ask" data-on={x.id === st.qid} onClick={() => pick(x.id)}>
              {x.ask}
            </button>
          ))}
        </div>
        <div className="q-answer">
          <div className="q-call" aria-label="MCP calls">
            {q.calls.map(c => <code key={c}>{c}</code>)}
          </div>
          <div className="q-tabs">
            <div className="q-tablist" role="tablist" aria-label="Which store answered" onKeyDown={e => arrowNav(e, '.q-tab')}>
              {(['graph', 'memory', 'knowledge'] as Store[]).map(s => (
                <button key={s} type="button" role="tab" id={`q-tab-${s}`} aria-controls="q-panel" aria-selected={tab === s} tabIndex={tab === s ? 0 : -1} data-on={tab === s} data-store={s} className="q-tab" onClick={() => setTab(s)}>
                  {STORE_LABEL[s].name} <span className="q-count">{counts[s]}</span>
                </button>
              ))}
            </div>
            <div className="q-tabs-r" data-off={tab !== 'graph' ? 'true' : undefined}>
              <ConfStops id="q-thr" value={threshold} onChange={setThreshold} />
            </div>
          </div>
          <div className="q-result" id="q-panel" role="tabpanel" tabIndex={-1} aria-labelledby={`q-tab-${tab}`} data-store={tab}>
            <div className="q-anim" key={`${st.qid}-${tab}-${st.nonce}`}>
              {tab !== 'graph' && <p className="q-kind">{STORE_LABEL[tab].name}: {STORE_LABEL[tab].kind}</p>}
              {counts[tab] === 0
                ? <p className="q-empty">Nothing from this store for this question. Try “Everything about applyDiscount”.</p>
                : tab === 'graph' ? <GraphView q={q} threshold={threshold} />
                : tab === 'memory' ? <MemoryView q={q} /> : <KnowledgeView q={q} />}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

function Hero(props: Parameters<typeof Playground>[0]) {
  return (
    <Section id="hero" hero>
      <div className="hero-head text-left">
        <span className="kicker">wicked-estate · v{__WICKED_ESTATE_VERSION__}</span>
        <h1 className="hero-h">{HEADLINE}</h1>
        {/* The approved sub-line, verbatim; each clause carries its store's colour, as the tabs below do. */}
        <p className="hero-sub">
          <span className="st st--graph">Your code as a graph</span>, <span className="st st--memory">what happened and what you told it</span>, and{' '}
          <span className="st st--knowledge">what your docs say</span>: a local CLI and MCP server your agents can query.
        </p>
        <ul className="facts" aria-label="At a glance">
          <li className="facts-store"><i className="sw sw--graph" aria-hidden />Code graph</li>
          <li className="facts-store"><i className="sw sw--memory" aria-hidden />Memory</li>
          <li className="facts-store"><i className="sw sw--knowledge" aria-hidden />Knowledge</li>
          <li className="facts-fact">Local CLI + MCP server</li>
          <li className="facts-fact">30 standard MCP tools</li>
          <li className="facts-fact">114 languages</li>
        </ul>
      </div>
      <div className="hero-play"><Playground {...props} /></div>
    </Section>
  )
}

// ── 2 · Three stores, one record (the benefits, on the stores they come from) ─
const STORES: { id: Store; name: string; kind: string; holds: string; from: string; ask: string[] }[] = [
  { id: 'graph', name: 'Code graph', kind: 'structured', holds: 'Symbols, calls, imports, and the edges grep never sees.', from: 'Indexing your repos, in 114 languages.', ask: ['BlastRadius', 'Path', 'ContextBundle'] },
  { id: 'memory', name: 'Memory', kind: 'told or witnessed', holds: 'What happened, and what you told it.', from: 'Agents and people capture it; reflection distills it.', ask: ['memory.recall', 'memory.capture'] },
  { id: 'knowledge', name: 'Knowledge', kind: 'read, then cited', holds: 'What your docs and wiki say, linked to the code.', from: 'Ingesting documents; hybrid keyword + vector recall.', ask: ['knowledge.recall', 'knowledge.ingest'] },
]
const BENEFITS: { h: string; p: string }[] = [
  { h: 'Know what breaks before you change it.', p: 'A code graph over 114 languages (depth varies by language), with the event and command links grep never sees.' },
  { h: 'Memory you can read, kept apart from knowledge.', p: 'What happened and what you told it, separate from what it learned from your code and docs. Memories about how you work can apply across projects; project memories stay with their project.' },
]
const NEWS: { b: string; rest: string }[] = [
  { b: 'Memories about how you work can apply across projects,', rest: ' using an explicit operator scope.' },
  { b: 'Did you mean…?', rest: ' A missed symbol name gets suggestions.' },
  { b: 'Prebuilt binaries for five platforms', rest: ' on every GitHub release.' },
]

function StorePreview({ id }: { id: Store }) {
  if (id === 'graph') {
    return (
      <div className="sc-prev">
        <p className="sc-prev-h">What depends on <code>{FOLLOW}</code></p>
        <ul className="sc-deps">
          {BLAST.filter(e => e.conf >= 0.5).map(e => (
            <li key={e.from}><code>{e.from}</code>{e.injected && <span className="q-chip q-chip--inj">injected</span>}<span className={confClass(e.conf)}>{e.conf.toFixed(1)}</span></li>
          ))}
        </ul>
      </div>
    )
  }
  if (id === 'memory') {
    const m = WHY[0]
    return (
      <div className="sc-prev">
        <p className="sc-prev-q">{m.text.split('. ')[0]}.</p>
        <span className="q-chips"><span className="q-chip q-chip--mem">{m.how}</span><span className="q-chip">kind: {m.kind}</span><span className="q-where">{m.when}</span></span>
      </div>
    )
  }
  const k = STACK[0]
  return (
    <div className="sc-prev">
      <p className="sc-prev-q">{k.text.split('. ')[0]}.</p>
      <span className="q-chips"><span className="q-chip q-chip--know">cited</span><span className="q-where">{k.source}</span></span>
    </div>
  )
}

function StoreCard({ s, open, onToggle }: { s: (typeof STORES)[number]; open: boolean; onToggle: () => void }) {
  return (
    <article className="sc" data-store={s.id} data-open={open}>
      <div className="sc-top">
        <h3 className="sc-name">{s.name}</h3>
        <span className="sc-kind">{s.kind}</span>
        <button type="button" className="sc-btn" aria-pressed={open} onClick={onToggle} aria-label={`${s.name}: what it holds about ${FOLLOW}`}>
          <code>{FOLLOW}</code><span aria-hidden>{open ? '×' : '→'}</span>
        </button>
      </div>
      <div className="sc-faces">
        <div className="sc-face sc-face--about" aria-hidden={open}>
          <p className="sc-holds">{s.holds}</p>
          <p className="sc-from">{s.from}</p>
          <div className="sc-tools">{s.ask.map(a => <code key={a} className="tag">{a}</code>)}</div>
        </div>
        <div className="sc-face sc-face--prev" aria-hidden={!open}><StorePreview id={s.id} /></div>
      </div>
    </article>
  )
}

function Stores() {
  const [open, setOpen] = useState<Store | null>(null)
  const toggle = (id: Store) => setOpen(o => (o === id ? null : id))
  const card = (id: Store) => <StoreCard s={STORES.find(s => s.id === id)!} open={open === id} onToggle={() => toggle(id)} />
  return (
    <Section id="stores" solid>
      <Head n="01" label="Three stores, one record" follow title="Three stores. One record."
        sub="Each answer says which store it came from, so a guess never reads like a fact and a note never reads like a doc." />
      <div id="why" className="stores" data-benefits>
        <div className="thread" aria-hidden>
          <span className="thread-node"><code>{FOLLOW}</code></span>
          <svg className="thread-lines" viewBox="0 0 600 34" preserveAspectRatio="none">
            <path className="tl tl--graph" d="M300 0 C300 22 100 12 100 34" />
            <path className="tl tl--memory" d="M300 0 C300 22 300 12 300 34" />
            <path className="tl tl--knowledge" d="M300 0 C300 22 500 12 500 34" />
          </svg>
        </div>
        {/* one grid: benefit 1 heads the code graph; benefit 2 spans memory and knowledge */}
        <div className="stores-grid">
          <div className="ben ben--graph"><h3>{BENEFITS[0].h}</h3><p>{BENEFITS[0].p}</p></div>
          {card('graph')}
          <div className="ben ben--pair"><h3>{BENEFITS[1].h}</h3><p>{BENEFITS[1].p}</p></div>
          {card('memory')}
          {card('knowledge')}
        </div>
        <div className="news">
          <h3 id="news-h">What’s new in 0.30–0.31</h3>
          <ul aria-labelledby="news-h" data-news>
            {NEWS.map(n => <li key={n.b}><b>{n.b}</b>{n.rest}</li>)}
          </ul>
          <ul className="news-facts">
            <li>Agents propose memories and rules. A person approves them.</li>
            <li>Erase a whole scope in one call: <code className="tag">memory.erase</code></li>
          </ul>
        </div>
      </div>
    </Section>
  )
}

// ── 3 · Edges grep never sees, and how sure each is ─────────────────────────
// ResolutionTier::default_confidence, crates/wicked-estate-core/src/edge.rs
const TIERS: { tier: string; conf: number; who: string }[] = [
  { tier: 'Parsed · SCIP · LSP · Compiler', conf: 1.0, who: 'read straight from the code or a precise indexer' },
  { tier: 'TSG', conf: 0.8, who: 'graph rules over the syntax tree' },
  { tier: 'ImportMap', conf: 0.6, who: 'followed through imports' },
  { tier: 'Heuristic', conf: 0.5, who: 'synthesizers and your TOML rules' },
  { tier: 'Tags', conf: 0.3, who: 'a tag-scan guess' },
]

type Mode = 'grep' | 'estate'

/** The event bus, made visible: grep sees the two call sites; estate adds the order.paid edges a TOML rule injects. */
function BusDiagram({ mode, vertical = false }: { mode: Mode; vertical?: boolean }) {
  // node centres; desktop is a left-to-right flow, the phone version stacks it
  const N = vertical
    ? { price: [80, 24], checkout: [220, 24], apply: [150, 96], paid: [220, 168], receipt: [80, 244], ledger: [220, 244] }
    : { price: [70, 26], checkout: [70, 124], apply: [260, 26], paid: [260, 124], receipt: [450, 96], ledger: [450, 152] }
  const w = vertical ? 300 : 520, h = vertical ? 268 : 170
  const nw = 124, nh = 28
  const node = (k: keyof typeof N, label: string, cls = '') => (
    <g className={`pv-n ${cls}`}>
      <rect x={N[k][0] - nw / 2} y={N[k][1] - nh / 2} width={nw} height={nh} rx="7" className={`q-node ${cls.includes('center') ? 'q-node--center' : ''} ${cls.includes('topic') ? 'q-node--inj' : ''}`} />
      <text x={N[k][0]} y={N[k][1] + 4.5} textAnchor="middle" className={`q-node-t ${cls.includes('center') ? 'q-node-t--center' : ''}`}>{label}</text>
    </g>
  )
  // an edge from a's border to b's border along the centre line
  const edge = (a: keyof typeof N, b: keyof typeof N, cls: string, i: number) => {
    const [x1, y1] = N[a], [x2, y2] = N[b]
    const dx = x2 - x1, dy = y2 - y1
    // where the centre line leaves a node box (both boxes are the same size)
    const s = Math.min(Math.abs(dx) > 0.01 ? (nw / 2 + 2) / Math.abs(dx) : 1, Math.abs(dy) > 0.01 ? (nh / 2 + 2) / Math.abs(dy) : 1)
    const sx = x1 + dx * s, sy = y1 + dy * s, ex = x2 - dx * s, ey = y2 - dy * s
    return (
      <g key={`${a}-${b}`} className={`pv-e ${cls}`} style={vars({ '--i': i, '--ox': `${sx}px`, '--oy': `${sy}px` })}>
        <line x1={sx} y1={sy} x2={ex} y2={ey} className={cls === 'pv-e--inj' ? 'q-edge q-edge--inj' : 'q-edge q-edge--call'} />
        {cls === 'pv-e--inj' && <circle className="q-dot" cx={sx} cy={sy} r="3.5" style={vars({ '--dx': `${ex - sx}px`, '--dy': `${ey - sy}px` })} />}
      </g>
    )
  }
  return (
    <svg viewBox={`0 0 ${w} ${h}`} className={`pv-svg${vertical ? ' pv-svg--v' : ' pv-svg--h'}`} data-mode={mode} role="img"
      aria-label={mode === 'grep'
        ? 'grep: POST /price and checkout() call applyDiscount; nothing links checkout() to sendReceipt() or updateLedger()'
        : 'estate: the two calls, plus checkout() emits order.paid, which sendReceipt() and updateLedger() consume (injected, Heuristic 0.5)'}>
      {edge('price', 'apply', 'pv-e--call', 0)}
      {edge('checkout', 'apply', 'pv-e--call', 0)}
      {edge('checkout', 'paid', 'pv-e--inj', 0)}
      {edge('paid', 'receipt', 'pv-e--inj', 1)}
      {edge('paid', 'ledger', 'pv-e--inj', 1)}
      {node('price', 'POST /price')}
      {node('checkout', 'checkout()')}
      {node('apply', FOLLOW, 'center')}
      {node('paid', 'order.paid', 'topic pv-x')}
      {node('receipt', 'sendReceipt()', 'pv-x')}
      {node('ledger', 'updateLedger()', 'pv-x')}
    </svg>
  )
}

function Provenance({ threshold, setThreshold }: { threshold: Stop; setThreshold: (s: Stop) => void }) {
  const [mode, setMode] = useState<Mode>('estate')
  const touched = useRef(false)
  const panel = useRef<HTMLDivElement>(null)
  // One-shot: off-screen at load, the diagram waits in grep mode and flips to estate when it enters.
  useEffect(() => {
    if (motionAllowed() && !onScreen(panel.current)) setMode('grep')
  }, [])
  useEnterOnce(panel, () => {
    if (!motionAllowed() || touched.current) return
    window.setTimeout(() => { if (!touched.current) setMode('estate') }, 650)
  })
  const choose = (m: Mode) => { touched.current = true; setMode(m) }
  const idx = [0.3, 0.5, 1.0].indexOf(threshold)
  const reach = BLAST.filter(e => e.conf >= threshold)
  return (
    <Section id="provenance">
      <Head n="02" label="Edges grep never sees" follow title="Edges grep never sees, each with its confidence."
        sub="A rule you write in TOML connects an event to its consumers. Every edge says how sure it is and where it came from." />
      <div className="pv">
        <div className="rock-panel pv-panel" ref={panel} data-mode={mode}>
          <div className="pv-bar">
            <span className="pv-bar-t">An event bus, made visible</span>
            <div className="pv-modes" role="group" aria-label="Who finds the edges">
              {(['grep', 'estate'] as Mode[]).map(m => (
                <button key={m} type="button" className="pv-mode" aria-pressed={mode === m} data-on={mode === m} onClick={() => choose(m)}>{m}</button>
              ))}
            </div>
          </div>
          <div className="pv-body">
            <BusDiagram mode={mode} />
            <BusDiagram mode={mode} vertical />
            <div className="pv-read" aria-live="polite">
              <p data-off={mode !== 'grep' ? 'true' : undefined}><b>grep finds 2 call sites.</b> Nothing links <code>checkout()</code> to the code that reacts to its event.</p>
              <p data-off={mode !== 'estate' ? 'true' : undefined}><b>The graph finds a third through an event.</b> <span className="q-chips"><span className="q-chip q-chip--inj">injected</span><span className="q-chip">Heuristic</span><span className="q-conf q-conf--mid">0.5</span><span className="q-where">rule: event-bus-consume</span></span></p>
            </div>
            <pre className="toml" data-hot={mode === 'estate' ? 'true' : undefined}><code><span className="toml-l">[[rule]]</span>
<span className="toml-l">name      = "event-bus-consume"</span>
<span className="toml-l">file_glob = "**/*.js"</span>
<span className="toml-l toml-l--hot">{`pattern   = "subscribe\\\\([\\"'](?P<topic>[\\\\w.]+)"`}</span></code></pre>
            <p className="pv-note">A new kind of edge is a rule block, not a code change. These land at 0.5, labelled with the rule’s name.</p>
          </div>
        </div>
        <div className="rock-panel pv-panel">
          <div className="pv-bar">
            <span className="pv-bar-t">How sure each edge is</span>
            <label htmlFor="thr-range" className="thr-l">Show edges ≥ <output htmlFor="thr-range" className="thr-v">{threshold.toFixed(1)}</output></label>
          </div>
          <div className="thr">
            <input id="thr-range" className="thr-range" type="range" min={0} max={2} step={1} value={idx}
              aria-valuetext={`${threshold.toFixed(1)}: ${threshold === 1 ? 'exact edges only' : threshold === 0.5 ? 'rules and above' : 'guesses too, labelled'}`}
              onChange={e => setThreshold(([0.3, 0.5, 1.0] as Stop[])[Number(e.target.value)])}
              style={vars({ '--p': `${idx * 50}%` })} />
            <div className="thr-ticks" aria-hidden><span>0.3 · guesses</span><span>0.5 · rules</span><span>1.0 · exact</span></div>
          </div>
          <table className="tiers">
            <tbody>
              {TIERS.map(t => (
                <tr key={t.tier} data-off={t.conf < threshold ? 'true' : undefined}>
                  <th scope="row"><span className="tiers-t">{t.tier}</span><span className="tiers-who">{t.who}</span></th>
                  <td className="tiers-bar"><span style={vars({ '--w': t.conf })} /></td>
                  <td className="tiers-n">{t.conf.toFixed(1)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="thr-read" aria-live="polite">
            <p><b>{reach.length} of {BLAST.length}</b> edges about <code>{FOLLOW}</code> shown.{' '}
              <span data-off={threshold !== 0.3 ? 'true' : undefined} className="thr-guess"><code>referralFlow()</code> appears as a tag-scan guess, unverified.</span>
            </p>
            <p className="thr-cap">When two tiers report the same edge, the higher one wins. A 0.3 guess is never handed to an agent as a 1.0 fact.</p>
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 4 · Everything an agent can call: an explorer ───────────────────────────
// MCP: crates/wicked-estate-mcp/src/lib.rs + src/tools/{memory,knowledge,proposals}.rs.
// CLI: crates/wicked-estate/src/main.rs (FEATURES.md §10).
type Row = { name: string; does: string; optional?: boolean }
type Group = { label: string; rows: Row[] }
type ToolSet = { id: string; name: string; note: string; mcp: boolean; groups: Group[] }
const TOOLSETS: ToolSet[] = [
  { id: 'graph', name: 'Graph', note: '12 MCP tools, plus SemanticSearch when embeddings are wired', mcp: true, groups: [
    { label: 'Find', rows: [
      { name: 'SearchEntity', does: 'Find symbols by name or kind' },
      { name: 'RetrieveEntity', does: 'One symbol, with its edges and notes' },
      { name: 'FetchContent', does: 'The source of a symbol or file' },
      { name: 'SemanticSearch', does: 'Search by meaning (when embeddings exist)', optional: true },
    ] },
    { label: 'Follow', rows: [
      { name: 'BlastRadius', does: 'Everything that depends on it' },
      { name: 'Lineage', does: 'Everything it depends on' },
      { name: 'Path', does: 'The route from A to B, hop by hop' },
      { name: 'TraverseGraph', does: 'A bounded walk out from a symbol' },
    ] },
    { label: 'Summarize', rows: [
      { name: 'ContextBundle', does: 'A prompt-ready pack for one symbol' },
      { name: 'RankHotspots', does: 'The most-connected code, to read first' },
      { name: 'Communities', does: 'The graph clustered into modules' },
    ] },
    { label: 'Rules', rows: [
      { name: 'RulesInventory', does: 'Rules engines and the code that calls them' },
      { name: 'rules.recall', does: 'Which of your rules apply here, cited' },
    ] },
  ] },
  { id: 'memory', name: 'Memory', note: '7 tools + 4 for proposals', mcp: true, groups: [
    { label: 'Use', rows: [
      { name: 'memory.capture', does: 'Save a memory under a scope' },
      { name: 'memory.recall', does: 'The memories that fit a token budget' },
      { name: 'memory.learn', does: 'Save a fact, linked to code' },
      { name: 'memory.list', does: 'Every memory in a scope' },
    ] },
    { label: 'Maintain', rows: [
      { name: 'memory.reflect', does: 'Distill a scope; scratch expires in 24 h' },
      { name: 'memory.coverage', does: 'Counts by tier and kind' },
      { name: 'memory.erase', does: 'Delete everything under a scope' },
    ] },
    { label: 'Propose, then approve', rows: [
      { name: 'proposal.submit', does: 'An agent proposes a memory or a rule' },
      { name: 'proposal.list', does: 'What waits for a person' },
      { name: 'proposal.approve', does: 'A person accepts it into the record' },
      { name: 'proposal.reject', does: 'A person declines it' },
    ] },
  ] },
  { id: 'knowledge', name: 'Knowledge', note: '7 tools', mcp: true, groups: [
    { label: 'Add', rows: [
      { name: 'knowledge.ingest', does: 'A document, split into searchable chunks' },
      { name: 'knowledge.write', does: 'One doc, section, chunk or concept' },
      { name: 'knowledge.relate', does: 'A typed link between two nodes' },
      { name: 'knowledge.relate_code', does: 'A link from a doc to code' },
    ] },
    { label: 'Ask', rows: [
      { name: 'knowledge.recall', does: 'Keyword + vector search, fused' },
      { name: 'knowledge.recall_about_code', does: 'Docs linked to given code' },
      { name: 'knowledge.coverage', does: 'Counts by class' },
    ] },
  ] },
  { id: 'cli', name: 'CLI', note: 'the same record, from your shell', mcp: false, groups: [
    { label: 'Keep it fresh', rows: [
      { name: 'watch', does: 'Re-index on every file change' },
      { name: 'changed-since', does: 'Symbols changed since a git SHA' },
      { name: 'subscribe', does: 'Follow the change log as JSON lines' },
    ] },
    { label: 'Sharpen it', rows: [
      { name: 'scip', does: 'Load a SCIP index for exact edges' },
      { name: 'index --embeddings', does: 'Add vectors for search by meaning' },
      { name: 'clusters', does: 'Group the graph into communities' },
    ] },
    { label: 'Many repos', rows: [
      { name: 'index --repo', does: 'Several repos in one database' },
      { name: 'cross-graph', does: 'Search across separate databases' },
      { name: 'export', does: 'The whole graph as JSON or NDJSON' },
    ] },
    { label: 'Infrastructure', rows: [
      { name: 'tfstate', does: 'Load live Terraform state' },
      { name: 'drift', does: 'Declared vs deployed resources' },
    ] },
    { label: 'Read the shape', rows: [
      { name: 'entrypoints', does: 'Code nothing calls' },
      { name: 'dead-code', does: 'Symbols with no edges at all' },
      { name: 'annotate', does: 'Notes that survive a re-index' },
      { name: 'by-requirement', does: 'Code that meets a requirement' },
    ] },
  ] },
]
/** Tools the playground has a scripted answer for: "see it" plays that question. Calls come from QUESTIONS. */
const SEE: Record<string, { q: string; tab?: Store; call: string }> = {
  BlastRadius: { q: 'blast', call: QUESTIONS[0].calls[0] },
  TraverseGraph: { q: 'event', call: QUESTIONS[1].calls[0] },
  'memory.recall': { q: 'why', call: QUESTIONS[2].calls[0] },
  'knowledge.recall': { q: 'stack', call: QUESTIONS[3].calls[0] },
  'knowledge.recall_about_code': { q: 'all', tab: 'knowledge', call: QUESTIONS[4].calls[2] },
}
const DEFAULT_SEL: Record<string, string> = { graph: 'BlastRadius', memory: 'memory.recall', knowledge: 'knowledge.recall', cli: 'watch' }
const SEARCH_CAP = 8

function countOf(t: ToolSet) {
  const all = t.groups.flatMap(g => g.rows)
  return { core: all.filter(r => !r.optional).length, optional: all.filter(r => r.optional).length }
}

function Toolface({ onSee }: { onSee: (q: string, tab?: Store) => void }) {
  const [set, setSet] = useState('graph')
  const [filter, setFilter] = useState('')
  const [sel, setSel] = useState('BlastRadius')
  const [openG, setOpenG] = useState<Record<string, string>>({})
  const [showAll, setShowAll] = useState(false)
  const f = filter.trim().toLowerCase()
  const hits = f
    ? TOOLSETS.flatMap(t => t.groups.flatMap(g => g.rows.filter(r => (r.name + ' ' + r.does).toLowerCase().includes(f)).map(r => ({ t, g, r }))))
    : []
  const current = TOOLSETS.find(t => t.id === set)!
  const find = (name: string) => {
    for (const t of TOOLSETS) for (const g of t.groups) for (const r of g.rows) if (r.name === name) return { t, g, r }
    return null
  }
  const picked = find(sel) ?? find(DEFAULT_SEL[set])!
  const openGroup = openG[set] ?? current.groups[0].label
  const choose = (id: string) => { setSet(id); setFilter(''); setSel(DEFAULT_SEL[id]); setShowAll(false) }
  const rowBtn = (r: Row) => (
    <button key={r.name} type="button" className="tl-row" aria-pressed={picked.r.name === r.name} data-on={picked.r.name === r.name} onClick={() => setSel(r.name)}>
      <code className="tl-name">{r.name}</code>
      {r.optional && <span className="tl-opt">when enabled</span>}
      {SEE[r.name] && <span className="tl-see-dot" aria-label="has a scripted answer" />}
      <span className="tl-does">{r.does}</span>
    </button>
  )
  const capped = hits.length > SEARCH_CAP && !showAll
  return (
    <Section id="toolface" solid>
      <Head n="03" label="What agents can call" follow title="Everything an agent can call."
        sub="30 MCP tools in three groups, and a CLI over the same record. Pick a group, or search them all." />
      <div className="tools" data-set={f ? 'search' : set}>
        <div className="tools-bar">
          <div className="tools-tabs" role="tablist" aria-label="Tool groups" onKeyDown={e => arrowNav(e, '.tools-tab')}>
            {TOOLSETS.map(t => {
              const c = countOf(t)
              const on = !f && set === t.id
              return (
                <button key={t.id} type="button" role="tab" aria-selected={on} tabIndex={on || (f && t.id === set) ? 0 : -1} data-on={on} className="tools-tab" onClick={() => choose(t.id)}
                  onFocus={e => e.currentTarget.scrollIntoView({ block: 'nearest', inline: 'nearest' })}>
                  {t.name}
                  {t.mcp && <span className="q-count">{c.core}{c.optional > 0 && <span className="q-count-opt"> +{c.optional}</span>}</span>}
                </button>
              )
            })}
          </div>
          <input className="tools-search" type="search" placeholder="Search tools, e.g. erase" aria-label="Search tools" value={filter}
            onChange={e => { setFilter(e.target.value); setShowAll(false) }} />
        </div>
        <p className="tools-note" aria-live="polite">
          {f ? (hits.length ? `${hits.length} ${hits.length === 1 ? 'tool matches' : 'tools match'} “${filter.trim()}”` : `No tool matches “${filter.trim()}”.`) : current.note}
          {!f && set === 'graph' && <span className="tools-note-x"> · SemanticSearch is the “+1”, not one of the 30</span>}
        </p>
        <div className="tools-body">
          <div className="tools-list" key={f ? 'search' : set} tabIndex={-1}>
            {f ? (
              // every hit is laid out; a phone shows the first eight until "Show all" (CSS), a desktop scrolls the fixed box
              <div className="tl-hits" data-all={showAll ? 'true' : undefined}>
                {hits.map(({ t, g, r }) => (
                  <div key={t.id + r.name} className="tl-hit"><span className="tl-hit-g">{t.name} · {g.label}</span>{rowBtn(r)}</div>
                ))}
                {capped && <button type="button" className="tl-more" onClick={() => setShowAll(true)}>Show all {hits.length}</button>}
              </div>
            ) : current.groups.map(g => (
              <div key={g.label} className="tl-group" data-open={openGroup === g.label}>
                <h3 className="tl-gl">{g.label}</h3>
                <button type="button" className="tl-acc" aria-expanded={openGroup === g.label} onClick={() => setOpenG(o => ({ ...o, [set]: g.label }))}>
                  <span>{g.label}</span>{' '}<span className="tl-acc-n">{g.rows.length}</span>
                </button>
                <div className="tl-rows">{g.rows.map(rowBtn)}</div>
              </div>
            ))}
          </div>
          <div className="tools-detail" aria-live="polite">
            <div className="td-in" key={picked.r.name}>
              <p className="td-path">{picked.t.mcp ? 'MCP tool' : 'CLI command'} · {picked.t.name} · {picked.g.label}</p>
              <p className="td-name"><code>{picked.r.name}</code>{picked.r.optional && <span className="tl-opt">when enabled</span>}</p>
              <p className="td-does">{picked.r.does}.</p>
              {picked.r.optional && <p className="td-x">Listed only when embeddings are wired, so it is the “+1” and not one of the 30.</p>}
              {SEE[picked.r.name] ? (
                <div className="td-see">
                  <code className="td-call">{SEE[picked.r.name].call}</code>
                  <button type="button" className="see-btn" onClick={() => onSee(SEE[picked.r.name].q, SEE[picked.r.name].tab)}>
                    See it answer <span aria-hidden>↑</span>
                  </button>
                </div>
              ) : (
                <p className="td-x">The playground has a scripted answer for <code>BlastRadius</code>, <code>TraverseGraph</code>, <code>memory.recall</code>, <code>knowledge.recall</code> and <code>knowledge.recall_about_code</code> (marked with a dot).</p>
              )}
            </div>
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 5 · Local by default ────────────────────────────────────────────────────
function Storage() {
  const [shared, setShared] = useState(false)
  const rows: [string, string, string][] = [
    ['Store', 'one graph.db file per repo', 'a Postgres you host'],
    ['Writers', 'one writer, no daemon', 'the whole team, concurrently'],
    ['Setup', 'nothing to run', 'WICKED_RUNTIME=team and WICKED_STORE_URL, on a --features postgres build'],
    ['MCP server', 'yes', 'not yet; it refuses to start rather than fall back'],
    ['Memory and knowledge', 'local SQLite', 'still local SQLite; not shared yet'],
  ]
  const mode = shared ? 'team' : 'solo'
  return (
    <Section id="storage">
      <Head n="04" label="Where it lives" title="Local by default." wide
        sub={<>The CLI and the MCP server keep the graph, memory and knowledge in local SQLite, with no database server to run: a catalog of 30 standard MCP tools, plus semantic search when enabled. A shared PostgreSQL code graph for a team is an optional CLI build; the MCP server, memory and knowledge do not use it yet.</>} />
      <div className="seg-row" role="group" aria-label="Storage">
        <button type="button" className="seg" aria-pressed={!shared} data-on={!shared} onClick={() => setShared(false)}>SQLite · solo</button>
        <button type="button" className="seg" aria-pressed={shared} data-on={shared} onClick={() => setShared(true)}>PostgreSQL · team</button>
        <span className="seg-fine">Read-only mode and unavailable stores reduce the set of tools offered.</span>
      </div>
      <div className="sd-grid">
        <div className="rock-panel sd" data-mode={mode} role="img"
          aria-label={shared
            ? 'Team: the code graph moves to a shared PostgreSQL on a CLI build with --features postgres; memory, knowledge and the MCP server stay local, and the MCP server refuses team mode'
            : 'Solo: one machine holds graph.db, memory and knowledge in SQLite, with the CLI and the MCP server attached'}>
          <div className="sd-box sd-box--local">
            <p className="sd-label">Your machine</p>
            <div className="sd-procs">
              <span className="sd-proc"><code>wicked-estate</code><small>CLI</small></span>
              <span className="sd-proc sd-proc--mcp"><code>wicked-estate-mcp</code><small>MCP server</small><em className="sd-refuse">refuses team mode</em></span>
            </div>
            <div className="sd-dbs">
              <span className="cyl cyl--graph sd-local-graph"><i />graph.db</span>
              <span className="cyl cyl--memory"><i />memory</span>
              <span className="cyl cyl--knowledge"><i />knowledge</span>
            </div>
            <p className="sd-foot">SQLite · nothing to run</p>
          </div>
          <div className="sd-link" aria-hidden><span /></div>
          <div className="sd-box sd-box--shared">
            <p className="sd-label">Shared PostgreSQL</p>
            <div className="sd-dbs sd-dbs--shared">
              <span className="cyl cyl--graph sd-shared-graph"><i />code graph</span>
              <span className="sd-empty">not used solo</span>
            </div>
            <p className="sd-foot">CLI build, <code>--features postgres</code></p>
          </div>
        </div>
        <div className="rock-panel p-0">
          <div className="store-cmd" data-storage-cmd>
            <span className="stk"><span data-off={shared ? 'true' : undefined}>wicked-estate index . --db graph.db</span><span data-off={!shared ? 'true' : undefined}>WICKED_RUNTIME=team wicked-estate index .</span></span>
          </div>
          <table className="store-t">
            <tbody>
              {rows.map(([k, solo, team]) => (
                <tr key={k}><th scope="row">{k}</th><td><span className="stk"><span data-off={shared ? 'true' : undefined}>{solo}</span><span data-off={!shared ? 'true' : undefined}>{team}</span></span></td></tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </Section>
  )
}

// ── 6 · Get started (its own island, after the SameGarden map) ──────────────
function CopyBtn({ text, label, onCopied }: { text: string; label: string; onCopied?: () => void }) {
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
      onCopied?.()
      if (timer.current) window.clearTimeout(timer.current)
      timer.current = window.setTimeout(() => setCopied(false), 2000)
    }
  }
  return (
    <button type="button" className="copy-btn" data-copied={String(copied)} onClick={copy} aria-label={`Copy: ${label}`}>
      <span className="stk"><span data-off={copied ? 'true' : undefined}>copy</span><span data-off={!copied ? 'true' : undefined}>copied ✓</span></span>
    </button>
  )
}

/** A command, broken only between tokens (never inside a flag), typed once when the terminal enters. */
function Cmd({ show, typed }: { show: string; typed: number }) {
  const lines = show.split('\n')
  // a flag keeps its value on its line ("-s project", "--db graph.db"); breaks fall only between tokens
  const words = (s: string) => {
    const parts = s.split(/( +)/)
    const out: string[] = []
    for (let i = 0; i < parts.length; i++) {
      const t = parts[i]
      if (/^-{1,2}\w/.test(t) && parts[i + 2] !== undefined && !/^-/.test(parts[i + 2]) && parts[i + 2] !== '\\') {
        out.push(t + parts[i + 1] + parts[i + 2]); i += 2
      } else out.push(t)
    }
    return out
  }
  const tok = (s: string, k: string) => words(s).map((t, i) => (/^ +$/.test(t) ? t : <span key={`${k}-${i}`} className={t === '\\' ? 'tok tok-cont' : 'tok'}>{t}</span>))
  let left = typed
  return (
    <span className="cmd">
      <span className="cmd-full">{lines.map((l, i) => <span key={i} className="cmd-line">{tok(l, `f${i}`)}</span>)}</span>
      <span className="cmd-type" aria-hidden>
        {lines.map((l, i) => {
          const n = Math.max(0, Math.min(l.length, left))
          left -= l.length + 1
          return <span key={i} className="cmd-line">{tok(l.slice(0, n), `t${i}`)}{n > 0 && n < l.length && <i className="caret" />}</span>
        })}
      </span>
    </span>
  )
}

const INSTALL_CMD = 'npx wicked-installer'
const STEPS: { n: number; label: string; cmd: string; show?: string; tip?: string }[] = [
  { n: 1, label: 'Install the CLI and the MCP server', cmd: 'cargo install wicked-estate wicked-estate-mcp' },
  { n: 2, label: 'Index your repo', cmd: 'wicked-estate index . --db graph.db' },
  { n: 3, label: 'Connect your agent (Claude Code shown)',
    cmd: 'claude mcp add wicked-estate -s project -- wicked-estate-mcp --db "$PWD/graph.db"',
    show: 'claude mcp add wicked-estate -s project \\\n  -- wicked-estate-mcp --db "$PWD/graph.db"',
    tip: 'Use an absolute database path: agent CLIs start from unpredictable directories.' },
]
const TYPE_MS = 1350

export function GetStarted() {
  const term = useRef<HTMLDivElement>(null)
  const total = STEPS.reduce((n, s) => n + (s.show ?? s.cmd).length, 0)
  const [typed, setTyped] = useState(total)
  const [done, setDone] = useState<Record<number, boolean>>({})
  const raf = useRef(0)
  // input ends the typing for good: a cancel before the terminal arrives must also stop it from starting
  const cancelled = useRef(false)
  const finish = useCallback(() => { cancelled.current = true; cancelAnimationFrame(raf.current); setTyped(total) }, [total])
  useEffect(() => {
    if (motionAllowed() && !onScreen(term.current)) setTyped(0)
    window.addEventListener('keydown', finish, { once: true })
    return () => { cancelAnimationFrame(raf.current); window.removeEventListener('keydown', finish) }
  }, [finish])
  useEnterOnce(term, () => {
    if (!motionAllowed() || cancelled.current) { setTyped(total); return }
    const t0 = performance.now()
    const tick = (t: number) => {
      if (cancelled.current) return
      const n = Math.min(total, Math.round(((t - t0) / TYPE_MS) * total))
      setTyped(n)
      if (n < total) raf.current = requestAnimationFrame(tick)
    }
    raf.current = requestAnimationFrame(tick)
  })
  let before = 0
  return (
    <Section id="get-started" solid>
      <Head n="05" label="Get started" title="Index a repo and ask it something."
        sub="Works with Claude Code, Antigravity, Codex, OpenCode and Pi." />
      <div className="gs" ref={term} onPointerDown={finish}>
        <div className="rock-panel gs-panel">
          <div className="gs-bar"><span className="gs-bar-t">Recommended: the whole family</span><span className="depth">npm</span></div>
          <div className="gs-body">
            <div className="gs-line">
              <span className="gs-cmd"><span className="gs-prompt" aria-hidden>$ </span><span className="cmd"><span className="cmd-line"><span className="tok">npx</span> <span className="tok">wicked-installer</span></span></span></span>
              <CopyBtn text={INSTALL_CMD} label={INSTALL_CMD} />
            </div>
            <p className="gs-p">Pick estate and any siblings, choose your agent CLIs, and it wires them up.</p>
          </div>
        </div>
        <div className="rock-panel gs-panel gs-term">
          <div className="gs-bar"><span className="gs-bar-t">Or just estate</span><span className="depth">crates.io</span></div>
          <ol className="gs-steps">
            {STEPS.map(s => {
              const show = s.show ?? s.cmd
              const mine = Math.max(0, Math.min(show.length, typed - before))
              before += show.length
              return (
                <li key={s.n} className="gs-step" data-done={done[s.n] ? 'true' : undefined}>
                  <span className="gs-n" aria-hidden>{s.n}</span>
                  <div className="gs-step-in">
                    <p className="gs-step-l">{s.label}</p>
                    <div className="gs-line">
                      <span className="gs-cmd"><span className="gs-prompt" aria-hidden>$ </span><Cmd show={show} typed={mine} /></span>
                      <CopyBtn text={s.cmd} label={s.cmd} onCopied={() => setDone(d => ({ ...d, [s.n]: true }))} />
                    </div>
                    {s.tip && <p className="gs-tip">{s.tip}</p>}
                  </div>
                </li>
              )
            })}
          </ol>
        </div>
      </div>
      <div className="gs-cta">
        <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-primary"><GitHubIcon /> GitHub</a>
        <a href="https://github.com/mikeparcewski/wicked-estate/tree/main/docs" target="_blank" rel="noreferrer" className="btn-outline">Documentation</a>
      </div>
    </Section>
  )
}

// The SameGarden map (an Astro component) renders between this island and GetStarted.
export default function Content() {
  const [st, setSt] = useState<PlayState>({ qid: 'blast', tab: 'graph', anim: 'intro', nonce: 0 })
  const [threshold, setThreshold] = useState<Stop>(0.5)
  // The hero's one-shot intro (CSS, from first paint) ends on any pointer or key input, or by itself.
  useEffect(() => {
    if (st.anim !== 'intro') return
    const end = () => setSt(s => (s.anim === 'intro' ? { ...s, anim: 'done' } : s))
    const t = window.setTimeout(end, 1900)
    window.addEventListener('pointerdown', end, { once: true })
    window.addEventListener('keydown', end, { once: true })
    return () => { window.clearTimeout(t); window.removeEventListener('pointerdown', end); window.removeEventListener('keydown', end) }
  }, [st.anim])
  const pick = useCallback((id: string, tab?: Store) => {
    const q = QUESTIONS.find(x => x.id === id)!
    setSt(s => ({ qid: id, tab: tab ?? q.first, anim: 'switch', nonce: s.nonce + 1 }))
  }, [])
  const setTab = useCallback((tab: Store) => setSt(s => ({ ...s, tab, anim: 'switch', nonce: s.nonce + 1 })), [])
  const changeThreshold = useCallback((t: Stop) => { setThreshold(t); setSt(s => (s.anim === 'intro' ? { ...s, anim: 'done' } : s)) }, [])
  const see = useCallback((id: string, tab?: Store) => {
    pick(id, tab)
    // after the pick renders: a desktop frame holds the whole hero (scroll to it, focus the question);
    // a phone stacks the questions above the answer, so it lands on the answer and focuses that
    window.setTimeout(() => {
      const behavior: ScrollBehavior = motionAllowed() ? 'smooth' : 'auto'
      const answer = document.querySelector<HTMLElement>('#query .q-answer')
      const desk = window.matchMedia('(min-width: 1024px)').matches
      if (desk || !answer) {
        window.scrollTo({ top: 0, behavior })
        document.querySelector<HTMLElement>('#query .q-ask[data-on="true"]')?.focus({ preventScroll: true })
      } else {
        const bar = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--topbar-h')) || 64
        window.scrollTo({ top: answer.getBoundingClientRect().top + window.scrollY - bar - 8, behavior })
        document.querySelector<HTMLElement>('#q-panel')?.focus({ preventScroll: true })
      }
    }, 30)
  }, [pick])
  return (
    <>
      <Hero st={st} pick={pick} setTab={setTab} threshold={threshold} setThreshold={changeThreshold} />
      <Stores />
      <Provenance threshold={threshold} setThreshold={changeThreshold} />
      <Toolface onSee={see} />
      <Storage />
    </>
  )
}
