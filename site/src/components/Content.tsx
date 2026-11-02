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
     A rule attaches its edge to the matched FILE and points it at a topic node, so
     the event is a second hop: BlastRadius on applyDiscount reaches checkout(), and
     BlastRadius on order.paid reaches the files that emit and consume it (Dependents
     = edges whose target is the symbol, crates/wicked-estate-store/src/sqlite.rs).
   - Memory: memory.recall returns scope, content, tier and score; capture tiers are
     working|episodic|semantic|procedural|archival; operator memories apply across
     projects (0.30.0). No other memory field is shown.
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

   MOTION: every animation is CSS (transform, opacity and SVG dash offsets), gated on
   html.motion-ok (set before first paint in index.astro only when the visitor
   allows motion). One-shot entrances key off the scene's data-inview mark; the calm
   idle loops (a pulse along the injected edges, a pulse down the store threads, the
   storage connector) key off data-live, which scenes.ts sets only while the scene is
   on screen, so they stop off screen. Every loop stays inside its own box and never
   changes layout. Pointer or key input ends the hero's intro. Reduced motion = the
   end state, with no loops.
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

/** The phone layout (global.css, max-width 760px): frames stack, and a few scenes trade a side-by-side
    view for a switch. */
function phoneLayout() {
  return typeof window !== 'undefined' && window.matchMedia('(max-width: 760px)').matches
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

const FOLLOW_LINE = `This page follows one function, ${FOLLOW}, through all three stores.`
function Follow() {
  return <span className="follow" title={FOLLOW_LINE}><i aria-hidden />example: <code>{FOLLOW}</code></span>
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
/** One edge of a scripted answer. `at` places its node in the answer's picture (viewBox FIG_W wide); `label`
    is the node's short name when `from` is a path. A rule's edge attaches to the matched file and points at a
    topic node, so an event is a second hop: `hop` names that topic, where it sits, and the node that emits it. */
interface GEdge {
  from: string; label?: string; to: string; via: string; tier: string; conf: number; where: string; injected?: boolean
  at: [number, number]; hop?: { label: string; at: [number, number]; via: string }
}
interface Mem { text: string; tier: 'semantic' | 'episodic'; scope: string }
interface Know { text: string; source: string; linked?: string }
/** The answer's picture: its height, the asked symbol's place, and context nodes that are not answer rows (the file
    that holds a function; `link` names the node it contains, a parsed structural edge). */
interface Fig { h: number; at: [number, number]; extra?: { label: string; at: [number, number]; link: string }[] }
interface Question { id: string; ask: string; calls: string[]; first: Store; center: string; fig: Fig; graph: GEdge[]; memory: Mem[]; knowledge: Know[]; grep?: string }
const FIG_W = 380

// BlastRadius on applyDiscount: its callers (checkout() lives in src/checkout/checkout.ts). A TOML rule attaches
// the emit edge to that FILE, pointing at the order.paid topic node; BlastRadius on order.paid (the second call)
// reaches the file that consumes it, at Heuristic 0.5 with the rule's name as its provenance.
const BLAST: GEdge[] = [
  { from: 'checkout()', to: 'applyDiscount', via: 'calls', tier: 'ImportMap', conf: 0.6, where: 'src/checkout/checkout.ts:41', at: [200, 90] },
  { from: 'POST /price', to: 'applyDiscount', via: 'calls', tier: 'SCIP', conf: 1.0, where: 'src/routes/price.ts:18', at: [200, 22] },
  { from: 'src/notify/receipt.ts', label: 'receipt.ts', to: '', via: 'consumes order.paid (checkout.ts emits it)', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true,
    at: [322, 160], hop: { label: 'order.paid', at: [322, 95], via: 'checkout.ts' } },
  { from: 'referralFlow()', to: 'applyDiscount', via: 'calls?', tier: 'Tags', conf: 0.3, where: 'tag-scan guess, unverified', at: [200, 158] },
]
// memory.recall answers with scope, content, tier and score: the chips show the tier and the scope
const WHY: Mem[] = [
  { text: 'A discount never takes a total below zero. Two stacked coupons produced a negative total in March and refunded the customer.', tier: 'semantic', scope: 'project:shop' },
  { text: 'Job 412 added the clamp and a test for it after the incident.', tier: 'episodic', scope: 'project:shop' },
]
const STACK: Know[] = [
  { text: 'Only one percentage coupon applies per cart. Fixed-amount coupons stack, up to the cart total.', source: 'docs/pricing.md › Coupons › Stacking', linked: 'applyDiscount' },
  { text: 'Coupon rules live in one function so the price route and checkout cannot disagree.', source: 'docs/adr/ADR-014-discounts.md › Decision', linked: 'applyDiscount' },
]
// TOML rules attach their edges to the matched files: the emitter and the consumers are files
const PAID: GEdge[] = [
  { from: 'src/checkout/checkout.ts', label: 'checkout.ts', to: 'order.paid', via: 'emits', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-emit', injected: true, at: [64, 75] },
  { from: 'src/notify/receipt.ts', label: 'receipt.ts', to: 'order.paid', via: 'consumes', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true, at: [318, 30] },
  { from: 'src/ledger/ledger.ts', label: 'ledger.ts', to: 'order.paid', via: 'consumes', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true, at: [318, 120] },
]

const BLAST_FIG: Fig = { h: 180, at: [62, 90], extra: [{ label: 'checkout.ts', at: [322, 30], link: 'checkout()' }] }
const QUESTIONS: Question[] = [
  { id: 'blast', ask: 'What breaks if I change applyDiscount?', calls: ['BlastRadius { symbol: "applyDiscount" }', 'BlastRadius { symbol: "order.paid" }'], first: 'graph', center: 'applyDiscount', fig: BLAST_FIG,
    graph: BLAST, memory: [], knowledge: [], grep: 'grep finds 2 call sites. The graph also follows the event checkout.ts emits.' },
  { id: 'event', ask: 'Who reacts when an order is paid?', calls: ['BlastRadius { symbol: "order.paid" }'], first: 'graph', center: 'order.paid', fig: { h: 150, at: [190, 75] },
    graph: PAID, memory: [], knowledge: [], grep: 'No function calls another here. A TOML rule adds these edges at index time.' },
  { id: 'why', ask: 'Why can a discount never go below zero?', calls: ['memory.recall { query: "discount below zero", scope: "project:shop" }'], first: 'memory', center: 'applyDiscount', fig: BLAST_FIG,
    graph: [], memory: WHY, knowledge: [] },
  { id: 'stack', ask: 'Can two coupons stack?', calls: ['knowledge.recall { query: "coupon stacking" }'], first: 'knowledge', center: 'applyDiscount', fig: BLAST_FIG,
    graph: [], memory: [], knowledge: STACK },
  { id: 'all', ask: 'Everything about applyDiscount', calls: ['BlastRadius', 'memory.recall', 'knowledge.recall_about_code'], first: 'graph', center: 'applyDiscount', fig: BLAST_FIG,
    graph: BLAST, memory: WHY, knowledge: STACK },
]

const STORE_LABEL: Record<Store, { name: string; kind: string }> = {
  graph: { name: 'Code graph', kind: 'structured' },
  memory: { name: 'Memory', kind: 'what happened, what you told it' },
  knowledge: { name: 'Knowledge', kind: 'read from your docs' },
}

/** The demo's confidence filter: one value, shared by the hero and the confidence scene. */
const STOPS = [1.0, 0.5, 0.3] as const
type Stop = (typeof STOPS)[number]

function confClass(c: number) { return c >= 0.8 ? 'q-conf q-conf--hi' : c >= 0.5 ? 'q-conf q-conf--mid' : 'q-conf q-conf--lo' }

type Anim = 'intro' | 'switch' | 'done'

// ── 1 · Hook + the query playground ─────────────────────────────────────────
/** A node box's half width for its label (mono 11px) */
const halfW = (label: string, bold = false) => Math.max(42, label.length * (bold ? 3.6 : 3.4) + 8)
/** A line between two node boxes, from border to border along their centre line, drawing out from `a`. */
function Seg({ a, b, aw, bw, cls, i, inj, hop = 0, from, to }: { a: [number, number]; b: [number, number]; aw: number; bw: number; cls: string; i: number; inj?: boolean; hop?: number; from: string; to: string }) {
  const [x1, y1] = a, [x2, y2] = b
  const dx = x2 - x1, dy = y2 - y1, hh = 13
  const cut = (hw: number) => Math.min(Math.abs(dx) > 0.01 ? hw / Math.abs(dx) : 1, Math.abs(dy) > 0.01 ? hh / Math.abs(dy) : 1)
  const s1 = cut(aw + 2), s2 = cut(bw + 2)
  const sx = x1 + dx * s1, sy = y1 + dy * s1, ex = x2 - dx * s2, ey = y2 - dy * s2
  return (
    <g className="q-seg" data-from={from} data-to={to} style={vars({ '--i': i, '--hop': hop, '--ox': `${sx}px`, '--oy': `${sy}px` })}>
      {inj && <line x1={sx} y1={sy} x2={ex} y2={ey} className="q-halo" />}
      <line x1={sx} y1={sy} x2={ex} y2={ey} className={cls} />
      {inj && <circle className="q-dot" cx={sx} cy={sy} r="3.5" style={vars({ '--dx': `${ex - sx}px`, '--dy': `${ey - sy}px` })} />}
    </g>
  )
}

/** An edge's relation; a parenthetical ("(checkout() emits it)") wraps as a unit, never leaving a word behind. */
function Via({ via }: { via: string }) {
  const at = via.indexOf(' (')
  if (at < 0) return <span className="q-via">{via}</span>
  return <span className="q-via">{via.slice(0, at)} <span className="q-via-x">{via.slice(at + 1)}</span></span>
}

function GraphView({ q, threshold, setThreshold }: { q: Question; threshold: Stop; setThreshold: (s: Stop) => void }) {
  // Confidence order (1.0 first) drives the draw order and the row order.
  const ordered = [...q.graph].sort((a, b) => b.conf - a.conf)
  const rank = new Map(ordered.map((e, i) => [e.from, i]))
  const rows = ordered.filter(e => e.conf >= threshold)
  const held = q.graph.length - rows.length
  const W = FIG_W, H = q.fig.h, C = q.fig.at, cw = halfW(q.center, true)
  // every node's place and label: the answer's edges, then the picture's context nodes
  const place = new Map<string, { at: [number, number]; label: string }>(q.graph.map(e => [e.from, { at: e.at, label: e.label ?? e.from }]))
  for (const x of q.fig.extra ?? []) place.set(x.label, { at: x.at, label: x.label })
  const emitter = q.graph.some(x => x.via === 'emits')
  const node = (at: [number, number], label: string, cls: string, bold = false) => {
    const hw = halfW(label, bold)
    return (
      <g className="q-en">
        <rect x={at[0] - hw} y={at[1] - 11} width={hw * 2} height="22" rx="6" className={cls} />
        <text x={at[0]} y={at[1] + 4} textAnchor="middle" className={bold ? 'q-node-t q-node-t--center' : 'q-node-t'}>{label}</text>
      </g>
    )
  }
  return (
    <div className="q-graphview">
      {/* the proof line leads the answer: first in reading order, above the picture it describes */}
      <div className="q-figure">
        {q.grep && <p className="q-note">{q.grep}</p>}
        <svg className="q-graph" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${q.center} and the ${rows.length} edges shown at ${threshold.toFixed(1)} or above`}>
          {q.graph.map((e, i) => {
            const on = e.conf >= threshold
            const r = rank.get(e.from) ?? i
            const label = e.label ?? e.from
            const cls = e.injected ? 'q-edge q-edge--inj' : e.conf < 0.5 ? 'q-edge q-edge--lo' : 'q-edge'
            const via = e.hop ? place.get(e.hop.via) : undefined
            const ctx = e.hop ? q.fig.extra?.find(x => x.label === e.hop!.via) : undefined
            const holder = ctx ? place.get(ctx.link) : undefined
            return (
              <g key={e.from} className="q-eg" data-on={on} style={vars({ '--i': r })}>
                {e.hop && via ? (
                  <>
                    {/* the event is the second hop: the file that holds the caller (a parsed "contains" edge), the
                        topic it emits, then the file that consumes it */}
                    {ctx && holder && <Seg a={holder.at} b={ctx.at} aw={halfW(holder.label)} bw={halfW(ctx.label)} cls="q-edge q-edge--struct" i={r} from={holder.label} to={ctx.label} />}
                    {ctx && node(ctx.at, ctx.label, 'q-node q-node--file')}
                    <Seg a={via.at} b={e.hop.at} aw={halfW(via.label)} bw={halfW(e.hop.label)} cls={cls} i={r} inj from={via.label} to={e.hop.label} />
                    <Seg a={e.hop.at} b={e.at} aw={halfW(e.hop.label)} bw={halfW(label)} cls={cls} i={r + 0.45} inj hop={1} from={e.hop.label} to={label} />
                    {node(e.hop.at, e.hop.label, 'q-node q-node--topic')}
                  </>
                ) : e.via === 'emits' ? (
                  // the emitter's edge runs into the topic, so a pulse travels emitter → topic → consumers
                  <Seg a={e.at} b={C} aw={halfW(label)} bw={cw} cls={cls} i={r} inj={e.injected} from={label} to={q.center} />
                ) : (
                  <Seg a={C} b={e.at} aw={cw} bw={halfW(label)} cls={cls} i={r + (emitter ? 0.45 : 0)} inj={e.injected} hop={emitter ? 1 : 0} from={q.center} to={label} />
                )}
                {node(e.at, label, e.injected ? 'q-node q-node--inj' : 'q-node')}
              </g>
            )
          })}
          {node(C, q.center, 'q-node q-node--center', true)}
        </svg>
      </div>
        <div className="q-rowwrap">
          <ul className="q-rows" aria-label="Edges">
            {rows.map(e => (
              <li key={e.from} className="q-row" data-inj={e.injected ? 'true' : undefined} style={vars({ '--i': rank.get(e.from) ?? 0 })}>
                {/* a phone drops the target: it is always the symbol asked about, named just above */}
                <span className="q-row-main"><b>{e.from}</b> <Via via={e.via} />{e.to && <span className="q-to"> {e.to}</span>}</span>
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
          {/* a phone keeps the confidence control with the edges it filters, not in the crowded tab row */}
          <div className="q-foot">
            <span className="q-foot-n">{held > 0 ? `${held} held back` : 'all shown'}</span>
            <ConfStops id="q-thr-ph" value={threshold} onChange={setThreshold} />
          </div>
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
            <span className="q-chip q-chip--mem" title="tier">{m.tier}</span>
            <span className="q-chip" title="scope">{m.scope}</span>
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
  const answer = useRef<HTMLDivElement>(null)
  // a tap on a question, on a phone, glides to the answer frame below the questions (keyboard picks stay put)
  const toAnswer = () => {
    const el = answer.current
    if (!el || !phoneLayout() || el.getBoundingClientRect().top < window.innerHeight * 0.55) return
    window.setTimeout(() => el.scrollIntoView({ block: 'start', behavior: motionAllowed() ? 'smooth' : 'auto' }), 20)
  }
  return (
    <div className="q-window" id="query" data-q={st.qid} data-anim={st.anim}>
      <div className="q-bar">
        <span className="q-dots" aria-hidden><i /><i /><i /></span>
        <span className="q-bar-t">wicked-estate-mcp<span className="q-bar-db"> · graph.db</span></span>
        <span className="q-bar-r"><span className="follow follow--line"><i aria-hidden /><span>This page follows one function, <code>{FOLLOW}</code>, through all three stores.</span></span><Follow /><span className="q-scripted">scripted demo<span className="q-scripted-x"> · pick a question</span></span></span>
      </div>
      <div className="q-body">
        <div className="q-asks" role="listbox" aria-label="Ask the record" onKeyDown={e => arrowNav(e, '.q-ask', true)}>
          <span className="q-k" aria-hidden>Ask</span>
          {QUESTIONS.map(x => (
            <button key={x.id} type="button" role="option" aria-selected={x.id === st.qid} tabIndex={x.id === st.qid ? 0 : -1} className="q-ask" data-on={x.id === st.qid}
              onClick={e => { pick(x.id); if (e.detail > 0) toAnswer() }}>
              {x.ask}
            </button>
          ))}
        </div>
        {/* a phone stacks the questions in the first frame and gives the answer a frame of its own */}
        <div className="q-answer ph-frame" ref={answer}>
          <p className="q-asked" aria-hidden><span>›</span> {q.ask}</p>
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
          <div className="q-result" id="q-panel" role="tabpanel" tabIndex={0} aria-labelledby={`q-tab-${tab}`} data-store={tab}>
            <div className="q-anim" key={`${st.qid}-${tab}-${st.nonce}`}>
              {tab !== 'graph' && <p className="q-kind">{STORE_LABEL[tab].name}: {STORE_LABEL[tab].kind}</p>}
              {counts[tab] === 0
                ? <p className="q-empty">Nothing from this store for this question. Try “Everything about applyDiscount”.</p>
                : tab === 'graph' ? <GraphView q={q} threshold={threshold} setThreshold={setThreshold} />
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
          <li className="facts-go"><a href="#get-started">Get started <span aria-hidden>↓</span></a></li>
        </ul>
      </div>
      <div className="hero-play"><Playground {...props} /></div>
    </Section>
  )
}

// ── 2 · Three stores, one record (the benefits, on the stores they come from) ─
const STORES: { id: Store; name: string; kind: string; holds: string; from: string; ask: string[] }[] = [
  { id: 'graph', name: 'Code graph', kind: 'structured', holds: 'Symbols, calls, imports, and the edges grep never sees.', from: 'Indexing your repos, in 114 languages.', ask: ['BlastRadius', 'Path', 'ContextBundle'] },
  { id: 'memory', name: 'Memory', kind: 'scoped', holds: 'What happened, and what you told it.', from: 'Agents and people capture it; reflection distills it.', ask: ['memory.recall', 'memory.capture'] },
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
        <p className="sc-prev-h">Around <code>{FOLLOW}</code></p>
        <ul className="sc-deps">
          {[...BLAST].sort((a, b) => b.conf - a.conf).filter(e => e.conf >= 0.5).map(e => (
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
        <span className="q-chips"><span className="q-chip q-chip--mem" title="tier">{m.tier}</span><span className="q-chip" title="scope">{m.scope}</span></span>
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

/** A phone-only view switch: a desktop shows every view side by side and hides this control. */
function PhoneSwitch<T extends string>({ label, value, options, onChange, cls = '' }: {
  label: string; value: T; options: { id: T; name: ReactNode; store?: Store }[]; onChange: (v: T) => void; cls?: string
}) {
  return (
    <div className={`ph-switch ${cls}`} role="group" aria-label={label}>
      {options.map(o => (
        <button key={o.id} type="button" className="ph-opt" aria-pressed={value === o.id} data-on={value === o.id} data-store={o.store} onClick={() => onChange(o.id)}>
          {o.name}
        </button>
      ))}
    </div>
  )
}

type StoreView = Store | 'news'

function Stores() {
  const [open, setOpen] = useState<Store | null>(null)
  const [view, setView] = useState<StoreView>('graph')
  const toggle = (id: Store) => setOpen(o => (o === id ? null : id))
  const card = (id: Store) => <StoreCard s={STORES.find(s => s.id === id)!} open={open === id} onToggle={() => toggle(id)} />
  return (
    <Section id="stores" solid>
      <Head n="01" label="Three stores, one record" follow title="Three stores. One record."
        sub="Each answer says which store it came from, so a guess never reads like a fact and a note never reads like a doc." />
      <div id="why" className="stores" data-benefits data-view={view}>
        {/* a phone keeps both benefits in sight above its switch; the switch then picks the card */}
        <div className="ben-heads">
          <h3 data-store="graph">{BENEFITS[0].h}</h3>
          <h3 data-store="memory">{BENEFITS[1].h}</h3>
        </div>
        <PhoneSwitch label="Show one store" value={view} onChange={setView} cls="st-views" options={[
          { id: 'graph', name: 'Code graph', store: 'graph' }, { id: 'memory', name: 'Memory', store: 'memory' },
          { id: 'knowledge', name: 'Knowledge', store: 'knowledge' }, { id: 'news', name: 'What’s new' },
        ]} />
        {/* one grid: the benefits head their columns (benefit 1 the code graph, benefit 2 memory and knowledge),
            then the followed symbol threads down into each store's card. Each view's wrapper is transparent to a
            desktop's grid; a phone stacks the views in one cell, sized to the largest. */}
        <div className="stores-grid">
          <div className="thread" aria-hidden>
            <span className="thread-node"><code>{FOLLOW}</code></span>
            <svg className="thread-lines" viewBox="0 0 600 34" preserveAspectRatio="none">
              {(['graph', 'memory', 'knowledge'] as Store[]).map((id, i) => {
                const d = `M300 0 C300 20 ${100 + i * 200} 14 ${100 + i * 200} 34`
                return (
                  <g key={id}>
                    <path className={`tl tl--${id}`} d={d} />
                    <path className={`tl-pulse tl--${id}`} d={d} pathLength={100} />
                  </g>
                )
              })}
            </svg>
          </div>
          <div className="st-v st-v--graph">
            <div className="ben ben--graph"><h3>{BENEFITS[0].h}</h3><p>{BENEFITS[0].p}</p></div>
            {card('graph')}
          </div>
          <div className="st-v st-v--pair">
            <div className="ben ben--pair"><h3>{BENEFITS[1].h}</h3><p>{BENEFITS[1].p}</p></div>
            <div className="st-pair">
              {card('memory')}
              {card('knowledge')}
            </div>
          </div>
          <div className="news">
            <h3 id="news-h">What’s new in 0.30–0.31</h3>
            <ul aria-labelledby="news-h" data-news>
              {NEWS.map(n => <li key={n.b}><b>{n.b}</b>{n.rest}</li>)}
            </ul>
          </div>
        </div>
      </div>
    </Section>
  )
}

// ── 3 · Edges grep never sees, and how sure each is ─────────────────────────
// ResolutionTier::default_confidence, crates/wicked-estate-core/src/edge.rs (defaults: a resolver may score an
// edge below its tier's default, as the import-map call above shows). No shipping path produces LSP-tier edges.
const TIERS: { tier: string; conf: number; who: string }[] = [
  { tier: 'Parsed · SCIP · Compiler', conf: 1.0, who: 'read straight from the code, a SCIP index or a compiler catalog' },
  { tier: 'TSG', conf: 0.8, who: 'graph rules over the syntax tree' },
  { tier: 'ImportMap', conf: 0.6, who: 'followed through imports' },
  { tier: 'Heuristic', conf: 0.5, who: 'synthesizers and your TOML rules' },
  { tier: 'Tags', conf: 0.3, who: 'a tag-scan guess' },
]

type Mode = 'grep' | 'estate'

/** The event bus, made visible: grep sees the two call sites; estate adds the order.paid edges TOML rules inject,
    from the emitting code to the topic and from the topic to the files that consume it. */
function BusDiagram({ mode, vertical = false }: { mode: Mode; vertical?: boolean }) {
  // node centres; desktop is a left-to-right flow, the phone version stacks it
  const N = vertical
    ? { price: [80, 16], checkout: [220, 16], apply: [80, 75], paid: [220, 75], receipt: [80, 134], ledger: [220, 134] }
    : { price: [70, 26], checkout: [70, 124], apply: [260, 26], paid: [260, 124], receipt: [450, 96], ledger: [450, 152] }
  const w = vertical ? 300 : 520, h = vertical ? 150 : 170
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
        {cls === 'pv-e--inj' && <line x1={sx} y1={sy} x2={ex} y2={ey} className="q-halo" />}
        <line x1={sx} y1={sy} x2={ex} y2={ey} className={cls === 'pv-e--inj' ? 'q-edge q-edge--inj' : 'q-edge q-edge--call'} />
        {cls === 'pv-e--inj' && <circle className="q-dot" cx={sx} cy={sy} r="3.5" style={vars({ '--dx': `${ex - sx}px`, '--dy': `${ey - sy}px` })} />}
      </g>
    )
  }
  return (
    <svg viewBox={`0 0 ${w} ${h}`} className={`pv-svg${vertical ? ' pv-svg--v' : ' pv-svg--h'}`} data-mode={mode} role="img"
      aria-label={mode === 'grep'
        ? 'grep: price.ts and checkout.ts call applyDiscount; nothing links checkout.ts to the files that consume its event'
        : 'estate: the two call sites, plus checkout.ts emits order.paid, which receipt.ts and ledger.ts consume (injected, Heuristic 0.5)'}>
      {edge('price', 'apply', 'pv-e--call', 0)}
      {edge('checkout', 'apply', 'pv-e--call', 0)}
      {edge('checkout', 'paid', 'pv-e--inj', 0)}
      {edge('paid', 'receipt', 'pv-e--inj', 1)}
      {edge('paid', 'ledger', 'pv-e--inj', 1)}
      {node('price', 'price.ts')}
      {node('checkout', 'checkout.ts')}
      {node('apply', FOLLOW, 'center')}
      {node('paid', 'order.paid', 'topic pv-x')}
      {node('receipt', 'receipt.ts', 'pv-x')}
      {node('ledger', 'ledger.ts', 'pv-x')}
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
              <p data-off={mode !== 'grep' ? 'true' : undefined}><b>grep finds 2 call sites.</b> Nothing links <code>checkout.ts</code> to the code that reacts to its event.</p>
              <p data-off={mode !== 'estate' ? 'true' : undefined}><b>The graph follows the event.</b> <code>checkout.ts</code> emits <code>order.paid</code>; a TOML rule links the files that consume it. <span className="q-chips"><span className="q-chip q-chip--inj">injected</span><span className="q-chip">Heuristic</span><span className="q-conf q-conf--mid">0.5</span><span className="q-where">rule: event-bus-consume</span></span></p>
            </div>
            {/* both halves of a consume rule: what it matches, and the edge it adds (the topic node's block is named) */}
            <div className="toml" data-hot={mode === 'estate' ? 'true' : undefined}>
              <pre className="toml-t"><code><span className="toml-l">[[rule]]</span>
<span className="toml-l">name = "event-bus-consume"</span>
<span className="toml-l">file_glob = "**/*.ts"</span>
<span className="toml-l toml-l--hot">{`pattern = "subscribe\\\\([\\"'](?P<topic>[\\\\w.]+)"`}</span></code></pre>
              <pre className="toml-t"><code><span className="toml-l toml-l--hot">[rule.emit_edge]</span>
<span className="toml-l">kind = "other:consumes"</span>
<span className="toml-l">target_id_template = "topic:{'{topic}'}"</span>
<span className="toml-l toml-c"># + [rule.emit_node] (the topic)</span></code></pre>
            </div>
            <p className="pv-note">An excerpt (docs/extractor-sdk.md): a new kind of edge is a rule block, not a code change. These land at 0.5, labelled with the rule’s name.</p>
          </div>
        </div>
        {/* a phone gives the confidence ladder a frame of its own */}
        <div className="rock-panel pv-panel ph-frame">
          <div className="pv-bar">
            <span className="pv-bar-t">Default confidence by tier</span>
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
            <p><b>{reach.length} of {BLAST.length}</b> edges in the <code>{FOLLOW}</code> answer shown.{' '}
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
/** Tools the playground has a scripted answer for: "see it" plays that question, and the detail pane previews
    the answer: each call it makes, with that call's own first results (so no result is shown under a call that
    does not return it). Calls and results come from the scripted data above. */
type PrevLine = { k: string; v?: string; tag?: 'inj' | 'mem' | 'know'; conf?: number; sans?: boolean }
type PrevCall = { call: string; lines: PrevLine[]; row?: boolean }
const firstSentence = (t: string) => t.split('. ')[0].replace(/\.$/, '') + '.'
const SEE: Record<string, { q: string; tab?: Store; calls: PrevCall[] }> = {
  BlastRadius: { q: 'blast', calls: [
    // BlastRadius on the function: its callers
    { call: QUESTIONS[0].calls[0], row: true, lines: [...BLAST].sort((a, b) => b.conf - a.conf).filter(e => e.conf >= 0.5 && !e.injected).map(e => ({ k: e.from, conf: e.conf })) },
    // BlastRadius on its event: the files that emit and consume it
    { call: QUESTIONS[0].calls[1], row: true, lines: PAID.slice(0, 2).map(e => ({ k: e.label ?? e.from, v: e.via, tag: 'inj', conf: e.conf })) },
  ] },
  'memory.recall': { q: 'why', calls: [{ call: QUESTIONS[2].calls[0],
    lines: WHY.slice(0, 1).map(m => ({ k: firstSentence(m.text), v: `${m.tier} · ${m.scope}`, tag: 'mem', sans: true })) }] },
  'knowledge.recall': { q: 'stack', calls: [{ call: QUESTIONS[3].calls[0],
    lines: STACK.slice(0, 1).map(k => ({ k: firstSentence(k.text), v: k.source, tag: 'know', sans: true })) }] },
  'knowledge.recall_about_code': { q: 'all', tab: 'knowledge', calls: [{ call: QUESTIONS[4].calls[2],
    lines: STACK.map(k => ({ k: k.source, v: `linked to ${k.linked}`, tag: 'know' })) }] },
}
const SEE_NAMES = Object.keys(SEE)
const DEFAULT_SEL: Record<string, string> = { graph: 'BlastRadius', memory: 'memory.recall', knowledge: 'knowledge.recall', cli: 'watch' }
/** A phone pages search results, three at a time, inside a fixed box (a desktop scrolls its fixed box). */
const PAGE = 3

function countOf(t: ToolSet) {
  const all = t.groups.flatMap(g => g.rows)
  return { core: all.filter(r => !r.optional).length, optional: all.filter(r => r.optional).length }
}

function Toolface({ onSee }: { onSee: (q: string, tab?: Store) => void }) {
  const [set, setSet] = useState('graph')
  const [filter, setFilter] = useState('')
  const [sel, setSel] = useState('BlastRadius')
  const [openG, setOpenG] = useState<Record<string, string>>({})
  const [page, setPage] = useState(0)
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
  // a phone shows one group at a time: by default the one that holds the selection, so list and detail agree
  const openGroup = openG[set] ?? (picked.t.id === set ? picked.g.label : current.groups[0].label)
  const gi = Math.max(0, current.groups.findIndex(g => g.label === openGroup))
  const ng = current.groups.length
  // stepping to another group selects its first tool, so the detail below always names a tool in view
  const stepGroup = (d: number) => {
    const g = current.groups[(gi + d + ng) % ng]
    setOpenG(o => ({ ...o, [set]: g.label }))
    setSel(g.rows[0].name)
  }
  const see = SEE[picked.r.name]
  const others = picked.g.rows.filter(r => r.name !== picked.r.name)
  const pages = Math.max(1, Math.ceil(hits.length / PAGE))
  const pg = Math.min(page, pages - 1)
  // the selection, its set and its open group always move together, so the list in view holds the tool the
  // detail names: picking a set opens its default tool's group; picking a tool (a search hit, a related tool)
  // opens that tool's set and group
  const select = (name: string) => {
    const hit = find(name)
    setSel(name)
    if (hit) { setSet(hit.t.id); setOpenG(o => ({ ...o, [hit.t.id]: hit.g.label })) }
  }
  const choose = (id: string) => {
    const def = find(DEFAULT_SEL[id])!
    setSet(id); setFilter(''); setSel(def.r.name); setPage(0)
    setOpenG(o => ({ ...o, [id]: def.g.label }))
  }
  // a related tool's button goes away with the detail it sits in: focus moves to that tool's row in the list
  const selectAndFocus = (name: string) => {
    // a search may hide the destination row: clear it, so the tool's set and group show with the row in them
    setFilter(''); setPage(0)
    select(name)
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`#toolface .tl-row[data-name="${name}"]`)?.focus())
  }
  const rowBtn = (r: Row) => (
    <button key={r.name} type="button" className="tl-row" data-name={r.name} aria-pressed={picked.r.name === r.name} data-on={picked.r.name === r.name} onClick={() => select(r.name)}>
      <code className="tl-name">{r.name}</code>
      {r.optional && <span className="tl-opt">when enabled</span>}
      {SEE[r.name] && <span className="tl-see-dot" aria-label="has a scripted answer" />}
      <span className="tl-does">{r.does}</span>
    </button>
  )
  return (
    <Section id="toolface" solid>
      <Head n="03" label="What agents can call" follow title="Everything an agent can call."
        sub="A catalog of 30 standard MCP tools in three groups, plus semantic search when enabled, and a CLI over the same record." />
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
            onChange={e => { setFilter(e.target.value); setPage(0) }} />
        </div>
        <p className="tools-note" aria-live="polite">
          {f ? (hits.length ? `${hits.length} ${hits.length === 1 ? 'tool matches' : 'tools match'} “${filter.trim()}”` : `No tool matches “${filter.trim()}”.`) : current.note}
          {!f && set === 'graph' && <span className="tools-note-x"> · SemanticSearch is the “+1”, not one of the 30</span>}
        </p>
        {/* a phone shows one group (or one page of matches) at a time, stepped here, so the box never grows */}
        <div className="tl-step" role="group" aria-label={f ? 'Pages of matches' : 'Tool groups in this set'} data-few={f && hits.length <= PAGE ? 'true' : undefined}>
          <button type="button" className="tl-step-b" aria-label={f ? 'Previous matches' : 'Previous group'} onClick={() => (f ? setPage((pg - 1 + pages) % pages) : stepGroup(-1))}><span aria-hidden>‹</span></button>
          <p className="tl-step-t" aria-live="polite">
            {f ? (hits.length === 0 ? <>No tool matches “{filter.trim()}”.</>
                  : hits.length <= PAGE ? <><b>{hits.length}</b> {hits.length === 1 ? 'tool matches' : 'tools match'}</>
                  : <><b>{`${pg * PAGE + 1}–${Math.min(hits.length, (pg + 1) * PAGE)}`}</b> of {hits.length}</>)
               : <><b>{openGroup}</b> <span>{gi + 1} of {ng}</span></>}
          </p>
          <button type="button" className="tl-step-b" aria-label={f ? 'Next matches' : 'Next group'} onClick={() => (f ? setPage((pg + 1) % pages) : stepGroup(1))}><span aria-hidden>›</span></button>
        </div>
        <div className="tools-body">
          <div className="tools-list" key={f ? 'search' : set} tabIndex={-1}>
            {f ? (
              // every hit is laid out; a desktop scrolls the fixed box, a phone shows the current page of four
              <div className="tl-hits">
                {hits.map(({ t, g, r }, i) => (
                  <div key={t.id + r.name} className="tl-hit" data-cur={Math.floor(i / PAGE) === pg ? 'true' : undefined}><span className="tl-hit-g">{t.name} · {g.label}</span>{rowBtn(r)}</div>
                ))}
              </div>
            ) : current.groups.map(g => (
              <div key={g.label} className="tl-group" data-open={openGroup === g.label}>
                <h3 className="tl-gl">{g.label}</h3>
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
              {see ? (
                <div className="td-see">
                  {/* the scripted answer's first lines, so the pane shows what "see it answer" plays */}
                  <div className="td-prev" aria-label="Scripted answer, first lines">
                    {see.calls.map(c => (
                      <div key={c.call} className="td-callset">
                        <code className="td-call">{c.call}</code>
                        <ul className="td-lines" data-row={c.row ? 'true' : undefined}>
                          {c.lines.map((l, i) => (
                            <li key={l.k} style={vars({ '--i': i })} data-tag={l.tag} data-sans={l.sans ? 'true' : undefined}>
                              <span className="td-k">{l.k}</span>
                              {l.v && <span className="td-v">{l.v}</span>}
                              {l.conf !== undefined && <span className={confClass(l.conf)}>{l.conf.toFixed(1)}</span>}
                            </li>
                          ))}
                        </ul>
                      </div>
                    ))}
                  </div>
                  <button type="button" className="see-btn" onClick={() => onSee(see.q, see.tab)}>
                    See it answer <span aria-hidden>↑</span>
                  </button>
                </div>
              ) : !picked.r.optional && (
                <>
                  <p className="td-x td-x-desk">The playground has a scripted answer for {SEE_NAMES.map((n, i) => (
                    <span key={n}>{i > 0 && (i === SEE_NAMES.length - 1 ? ' and ' : ', ')}<code>{n}</code></span>
                  ))} (marked with a dot).</p>
                  {others.length > 0 && (
                    <div className="td-more">
                      <span className="td-more-l">More in {picked.g.label}</span>
                      {others.map(r => (
                        <button key={r.name} type="button" className="td-more-b" onClick={() => selectAndFocus(r.name)}>
                          <code>{r.name}</code>{SEE[r.name] && <span className="tl-see-dot" aria-label="has a scripted answer" />}
                        </button>
                      ))}
                    </div>
                  )}
                </>
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
    ['Store', 'a local graph.db (one repo, or several with --repo)', 'a Postgres you host'],
    ['Writers', 'one writer, no daemon', 'the whole team, concurrently'],
    ['Setup', 'nothing to run', 'WICKED_RUNTIME=team and WICKED_STORE_URL, on a --features postgres build'],
    ['MCP server', 'serves it', 'not yet; it refuses team mode rather than fall back'],
    ['Memory and knowledge', 'local SQLite', 'still local SQLite; not shared yet'],
  ]
  const mode = shared ? 'team' : 'solo'
  return (
    <Section id="storage">
      <Head n="04" label="Where it lives" title="Local by default." wide
        sub={<>The CLI and the MCP server keep the graph, memory and knowledge in local SQLite, with no database server to run. <span className="sub-fine">A shared PostgreSQL code graph for a team is an optional CLI build; the MCP server, memory and knowledge do not use it yet.</span></>} />
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
          {/* the code graph travels the connector to the shared Postgres; in team mode its writes keep flowing */}
          <div className="sd-link" aria-hidden><span><b className="sd-trip" key={mode} /><b className="sd-flow" /></span></div>
          <div className="sd-box sd-box--shared">
            <p className="sd-label">Shared PostgreSQL</p>
            <div className="sd-dbs sd-dbs--shared">
              {/* solo shows the team picture as a dimmed outline */}
              <span className="cyl cyl--graph sd-shared-graph"><i />code graph</span>
              <span className="sd-empty"><span className="stk"><span data-off={shared ? 'true' : undefined}>not used solo</span><span data-off={!shared ? 'true' : undefined}>the whole team, concurrently</span></span></span>
            </div>
            <p className="sd-foot">CLI build, <code>--features postgres</code></p>
          </div>
        </div>
        {/* a phone gives the details a frame of their own, with the same switch at hand */}
        <div className="rock-panel p-0 sd-detail ph-frame">
          <PhoneSwitch label="Storage details" value={mode} onChange={m => setShared(m === 'team')} cls="sd-views" options={[
            { id: 'solo', name: 'SQLite · solo' }, { id: 'team', name: 'PostgreSQL · team' },
          ]} />
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
          {/* a phone reads this line with the details (a desktop shows it beside the switch) */}
          <p className="sd-fine">Read-only mode and unavailable stores reduce the set of tools offered.</p>
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

/** A command, broken only between tokens (never inside a flag), typed once when the terminal enters. The last
    command keeps a caret at its end once typed (it blinks while the scene is on screen). */
function Cmd({ show, typed, last = false }: { show: string; typed: number; last?: boolean }) {
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
          const end = last && i === lines.length - 1 && n === l.length
          return <span key={i} className="cmd-line">{tok(l.slice(0, n), `t${i}`)}{n > 0 && n < l.length && <i className="caret" />}{end && <i className="caret caret--idle" />}</span>
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
  const [path, setPath] = useState<'npm' | 'cargo'>('npm')
  const raf = useRef(0)
  // input ends the typing for good: a cancel before the terminal arrives must also stop it from starting
  const cancelled = useRef(false)
  const finish = useCallback(() => { cancelled.current = true; cancelAnimationFrame(raf.current); setTyped(total) }, [total])
  useEffect(() => {
    if (motionAllowed() && !onScreen(term.current)) setTyped(0)
    window.addEventListener('keydown', finish, { once: true })
    return () => { cancelAnimationFrame(raf.current); window.removeEventListener('keydown', finish) }
  }, [finish])
  // types once, when the terminal is first seen: on arrival, or (a phone shows one install path at a time)
  // when its path is picked
  const played = useRef(false)
  const entered = useRef(false)
  const play = () => {
    if (played.current) return
    played.current = true
    if (!motionAllowed() || cancelled.current) { setTyped(total); return }
    const t0 = performance.now()
    const tick = (t: number) => {
      if (cancelled.current) return
      const n = Math.min(total, Math.round(((t - t0) / TYPE_MS) * total))
      setTyped(n)
      if (n < total) raf.current = requestAnimationFrame(tick)
    }
    raf.current = requestAnimationFrame(tick)
  }
  const termShown = () => {
    const el = term.current?.querySelector<HTMLElement>('.gs-term')
    return !!el && getComputedStyle(el).visibility !== 'hidden'
  }
  useEnterOnce(term, () => { entered.current = true; if (termShown()) play() })
  useEffect(() => { if (path === 'cargo' && entered.current) play() }, [path])
  // reduced motion switched on during the visit: any typing stops, and every command shows whole
  useEffect(() => {
    const rm = window.matchMedia('(prefers-reduced-motion: reduce)')
    const on = () => { if (rm.matches) finish() }
    rm.addEventListener?.('change', on)
    return () => rm.removeEventListener?.('change', on)
  }, [finish])
  // widening a phone past its layout shows both paths: a terminal it hid types then
  useEffect(() => {
    const mq = window.matchMedia('(max-width: 760px)')
    const on = () => requestAnimationFrame(() => { if (entered.current && termShown()) play() })
    mq.addEventListener?.('change', on)
    return () => mq.removeEventListener?.('change', on)
  }, [])
  let before = 0
  return (
    <Section id="get-started" solid>
      <Head n="05" label="Get started" title="Index a repo and ask it something."
        sub="Works with Claude Code, Antigravity, Codex, OpenCode and Pi." />
      <PhoneSwitch label="Install path" value={path} onChange={setPath} cls="gs-views" options={[
        { id: 'npm', name: 'Recommended' }, { id: 'cargo', name: 'Or just estate' },
      ]} />
      <div className="gs" ref={term} onPointerDown={finish} data-view={path}>
        <div className="rock-panel gs-panel gs-npm">
          <div className="gs-bar"><span className="gs-bar-t">Recommended: the whole family</span><span className="depth">npm</span></div>
          <div className="gs-body">
            <div className="gs-line">
              <span className="gs-cmd"><span className="gs-prompt" aria-hidden>$ </span><span className="cmd"><span className="cmd-line"><span className="tok">npx</span> <span className="tok">wicked-installer</span></span></span></span>
              <CopyBtn text={INSTALL_CMD} label={INSTALL_CMD} />
            </div>
            {/* the approved line, as the three beats the installer walks through */}
            <ol className="gs-beats" aria-label="Pick estate and any siblings, choose your agent CLIs, and it wires them up.">
              <li><span className="gs-beat-n" aria-hidden>1</span>Pick estate and any siblings</li>
              <li><span className="gs-beat-n" aria-hidden>2</span>Choose your agent CLIs</li>
              <li><span className="gs-beat-n" aria-hidden>3</span>It wires them up</li>
            </ol>
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
                      <span className="gs-cmd"><span className="gs-prompt" aria-hidden>$ </span><Cmd show={show} typed={mine} last={s.n === STEPS.length} /></span>
                      <CopyBtn text={s.cmd} label={s.cmd} onCopied={() => setDone(d => ({ ...d, [s.n]: true }))} />
                    </div>
                    {s.tip && <p className="gs-tip">{s.tip}</p>}
                  </div>
                </li>
              )
            })}
          </ol>
        </div>
        {/* a desktop keeps the links in the left column, under the recommended path */}
        <div className="gs-cta">
          <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-primary"><GitHubIcon /> GitHub</a>
          <a href="https://github.com/mikeparcewski/wicked-estate/tree/main/docs" target="_blank" rel="noreferrer" className="btn-outline">Documentation</a>
        </div>
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
    // a phone stacks the questions and the controls above the answer, so it lands on the answer itself
    // (below the topbar) and focuses it
    window.setTimeout(() => {
      const behavior: ScrollBehavior = motionAllowed() ? 'smooth' : 'auto'
      const answer = document.querySelector<HTMLElement>('#q-panel')
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
