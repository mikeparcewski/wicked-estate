import { useState, useEffect, useRef } from 'react'

/* ────────────────────────────────────────────────────────────────────────────
   wicked-estate (we.wickedagile.com): memory and knowledge for coding agents,
   structured and unstructured.

   Shape: hook + a working query playground → three stores → injected edges and
   confidence → everything an agent can call (an explorer, not a wall) → solo or
   shared storage → (SameGarden, in index.astro) → get started.

   ACCURACY (estate 0.30.0), each grounded in the repo:
   - MCP tools: 12 graph tools always listed + SemanticSearch when embeddings are
     wired; 7 memory + 4 proposal tools; 7 knowledge tools = 30
     (crates/wicked-estate-mcp/src/lib.rs, src/tools/*.rs).
   - Tier confidences: ResolutionTier::default_confidence,
     crates/wicked-estate-core/src/edge.rs.
   - Injected edges: ExtraEdgeExtractor, plain-TOML rules, tier Heuristic (0.5),
     provenance = the rule name (crates/wicked-estate-extract/src/extra_edge.rs).
   - Memory kinds working|episode|entity|fact|skill|archive, scoped, erasable by
     subtree; operator memories apply across projects (0.30.0).
   - The playground is scripted: it shows the shape of real answers, it never
     runs a query.
   ──────────────────────────────────────────────────────────────────────────── */

// The hook. Two strings, so the operator can swap a variant in one place.
const HEADLINE = 'Memory and knowledge for your coding agents.'
const SUBLINE = 'Your code as a graph, what happened and what you told it, and what your docs say: one local binary your agents query over MCP.'

function GitHubIcon({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.02-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 012-.27c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  )
}

function Section({ id, solid = false, hero = false, children }: { id?: string; solid?: boolean; hero?: boolean; children: React.ReactNode }) {
  return (
    <section id={id} className={`strata${solid ? ' strata-solid' : ''}${hero ? ' strata--hero' : ''} px-4 sm:px-7`}>
      <div className="max-w-6xl mx-auto w-full">{children}</div>
    </section>
  )
}

function Head({ title, sub }: { title: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <div className="mb-7 w-full text-left">
      <h2 className="font-display text-3xl sm:text-[2.5rem] font-black text-ink leading-[0.98]">{title}</h2>
      {sub && <p className="mt-3 text-base sm:text-lg text-muted font-sans max-w-3xl leading-snug">{sub}</p>}
    </div>
  )
}

// ── 1 · Hook + the query playground ─────────────────────────────────────────
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

const QUESTIONS: Question[] = [
  { id: 'blast', ask: 'What breaks if I change applyDiscount?', calls: ['BlastRadius { symbol: "applyDiscount" }'], first: 'graph', center: 'applyDiscount',
    graph: BLAST, memory: [], knowledge: [], grep: 'grep finds 2 call sites. The graph finds a third through an event.' },
  { id: 'event', ask: 'Who reacts when an order is paid?', calls: ['TraverseGraph { symbol: "topic:order.paid" }'], first: 'graph', center: 'order.paid',
    graph: [
      { from: 'checkout()', to: 'order.paid', via: 'emits', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-emit', injected: true },
      { from: 'sendReceipt()', to: 'order.paid', via: 'consumes', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true },
      { from: 'updateLedger()', to: 'order.paid', via: 'consumes', tier: 'Heuristic', conf: 0.5, where: 'rule: event-bus-consume', injected: true },
    ], memory: [], knowledge: [], grep: 'No function calls another here. A TOML rule adds these edges at index time.' },
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

function confClass(c: number) { return c >= 0.8 ? 'q-conf q-conf--hi' : c >= 0.5 ? 'q-conf q-conf--mid' : 'q-conf q-conf--lo' }

function GraphView({ q, showGuesses }: { q: Question; showGuesses: boolean }) {
  const edges = q.graph.filter(e => showGuesses || e.conf >= 0.5)
  const hidden = q.graph.length - edges.length
  // A small radial picture: the asked-about node in the middle, its neighbours around it.
  const W = 360, H = 150, cx = 180, cy = 75
  const pts = q.graph.map((_, i) => {
    const a = (Math.PI * 2 * i) / q.graph.length - Math.PI / 2
    return { x: cx + Math.cos(a) * 124, y: cy + Math.sin(a) * 54 }
  })
  return (
    <div className="q-graphview">
      <svg className="q-graph" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${q.center} and the ${edges.length} edges shown`}>
        {q.graph.map((e, i) => {
          const on = showGuesses || e.conf >= 0.5
          return (
            <g key={e.from} opacity={on ? 1 : 0.12}>
              <line x1={pts[i].x} y1={pts[i].y} x2={cx} y2={cy} className={e.injected ? 'q-edge q-edge--inj' : e.conf < 0.5 ? 'q-edge q-edge--lo' : 'q-edge'} />
              <rect x={pts[i].x - 46} y={pts[i].y - 10} width="92" height="20" rx="6" className="q-node" />
              <text x={pts[i].x} y={pts[i].y + 4} textAnchor="middle" className="q-node-t">{e.from}</text>
            </g>
          )
        })}
        <rect x={cx - 52} y={cy - 12} width="104" height="24" rx="7" className="q-node q-node--center" />
        <text x={cx} y={cy + 4} textAnchor="middle" className="q-node-t q-node-t--center">{q.center}</text>
      </svg>
      <div>
      <ul className="q-rows" aria-label="Edges">
        {edges.map(e => (
          <li key={e.from} className="q-row">
            <span className="q-row-main"><b>{e.from}</b> <span className="text-muted">{e.via}</span> {e.to}</span>
            <span className="q-chips">
              {e.injected && <span className="q-chip q-chip--inj">injected</span>}
              <span className="q-chip">{e.tier}</span>
              <span className={confClass(e.conf)}>{e.conf.toFixed(1)}</span>
            </span>
            <span className="q-where">{e.where}</span>
          </li>
        ))}
      </ul>
      {hidden > 0 && <p className="q-hidden" data-hidden={hidden}>{hidden} guess below 0.5 hidden. Turn on “Show guesses” to see it, labelled.</p>}
      {q.grep && <p className="q-note">{q.grep}</p>}
      </div>
    </div>
  )
}

function MemoryView({ q }: { q: Question }) {
  return (
    <ul className="q-cards">
      {q.memory.map(m => (
        <li key={m.text} className="q-card">
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
      {q.knowledge.map(k => (
        <li key={k.source} className="q-card">
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

function Playground() {
  const [qid, setQid] = useState('blast')
  const [tab, setTab] = useState<Store>('graph')
  const [showGuesses, setShowGuesses] = useState(false)
  const q = QUESTIONS.find(x => x.id === qid)!
  const counts: Record<Store, number> = { graph: q.graph.length, memory: q.memory.length, knowledge: q.knowledge.length }
  const pick = (next: Question) => { setQid(next.id); setTab(next.first) }
  return (
    <div className="q-window" id="query" data-q={qid}>
      <div className="q-bar">
        <span className="q-dots" aria-hidden><i /><i /><i /></span>
        <span className="q-bar-t">wicked-estate-mcp · graph.db</span>
        <span className="q-bar-r">scripted demo · pick a question</span>
      </div>
      <div className="q-body">
        <div className="q-asks" role="listbox" aria-label="Ask the record">
          <span className="q-k">Ask</span>
          {QUESTIONS.map(x => (
            <button key={x.id} type="button" role="option" aria-selected={x.id === qid} className="q-ask" data-on={x.id === qid} onClick={() => pick(x)}>
              {x.ask}
            </button>
          ))}
        </div>
        <div className="q-answer">
          <div className="q-call" aria-label="MCP calls">
            {q.calls.map(c => <code key={c}>{c}</code>)}
          </div>
          <div className="q-tabs" role="tablist" aria-label="Which store answered">
            {(['graph', 'memory', 'knowledge'] as Store[]).map(s => (
              <button key={s} type="button" role="tab" aria-selected={tab === s} data-on={tab === s} data-store={s} className="q-tab" onClick={() => setTab(s)}>
                {STORE_LABEL[s].name} <span className="q-count">{counts[s]}</span>
              </button>
            ))}
            {tab === 'graph' && (
              <label className="q-toggle">
                <input type="checkbox" checked={showGuesses} onChange={e => setShowGuesses(e.target.checked)} /> Show guesses
              </label>
            )}
          </div>
          <div className="q-result" role="tabpanel" aria-label={STORE_LABEL[tab].name}>
            <p className="q-kind">{STORE_LABEL[tab].name}: {STORE_LABEL[tab].kind}</p>
            {counts[tab] === 0
              ? <p className="q-empty">Nothing from this store for this question. Try “Everything about applyDiscount”.</p>
              : tab === 'graph' ? <GraphView q={q} showGuesses={showGuesses} />
              : tab === 'memory' ? <MemoryView q={q} /> : <KnowledgeView q={q} />}
          </div>
        </div>
      </div>
    </div>
  )
}

function Hero() {
  return (
    <Section hero>
      <div className="hero-head text-left">
        <span className="kicker">wicked-estate · v{__WICKED_ESTATE_VERSION__}</span>
        <h1 className="mt-3 font-display font-black text-ink leading-[0.95] text-[clamp(2rem,3.7vw,3rem)]" style={{ fontStretch: '112%' }}>
          {HEADLINE}
        </h1>
        <p className="mt-2.5 text-base sm:text-lg text-muted font-sans max-w-3xl leading-snug">{SUBLINE}</p>
      </div>
      <div className="mt-4"><Playground /></div>
    </Section>
  )
}

// ── 2 · Three stores ────────────────────────────────────────────────────────
const STORES: { name: string; kind: string; holds: string; from: string; ask: string[] }[] = [
  { name: 'Code graph', kind: 'structured', holds: 'Symbols, calls, imports, and the edges grep never sees.', from: 'Indexing your repos, in 114 languages.', ask: ['BlastRadius', 'Path', 'ContextBundle'] },
  { name: 'Memory', kind: 'told or witnessed', holds: 'What happened, and what you told it.', from: 'Agents and people capture it; reflection distills it.', ask: ['memory.recall', 'memory.capture'] },
  { name: 'Knowledge', kind: 'read, then cited', holds: 'What your docs and wiki say, linked to the code.', from: 'Ingesting documents; hybrid keyword + vector recall.', ask: ['knowledge.recall', 'knowledge.ingest'] },
]

function Stores() {
  return (
    <Section id="stores" solid>
      <Head title="Three stores. One record." sub="Each answer says which store it came from, so a guess never reads like a fact and a note never reads like a doc." />
      <div className="grid md:grid-cols-3 gap-3">
        {STORES.map(s => (
          <div key={s.name} className="rock-panel p-5 flex flex-col gap-3">
            <div className="flex items-baseline justify-between gap-2">
              <h3 className="font-display text-xl font-black text-ink">{s.name}</h3>
              <span className="tag tag-accent">{s.kind}</span>
            </div>
            <p className="text-ink font-sans leading-snug">{s.holds}</p>
            <p className="text-muted text-sm font-sans leading-snug">{s.from}</p>
            <div className="mt-auto flex flex-wrap gap-1.5">{s.ask.map(a => <code key={a} className="tag">{a}</code>)}</div>
          </div>
        ))}
      </div>
      <ul className="mt-4 grid md:grid-cols-3 gap-3 text-sm text-muted font-sans">
        <li className="fact-line">Agents propose memories and rules. A person approves them.</li>
        <li className="fact-line">Erase a whole scope in one call: <code className="tag">memory.erase</code></li>
        <li className="fact-line">Your own memories can apply across every project.</li>
      </ul>
    </Section>
  )
}

// ── 3 · Injected edges + confidence ─────────────────────────────────────────
// ResolutionTier::default_confidence, crates/wicked-estate-core/src/edge.rs
const TIERS: { tier: string; conf: number; who: string }[] = [
  { tier: 'Parsed · SCIP · LSP · Compiler', conf: 1.0, who: 'read straight from the code or a precise indexer' },
  { tier: 'TSG', conf: 0.8, who: 'graph rules over the syntax tree' },
  { tier: 'ImportMap', conf: 0.6, who: 'followed through imports' },
  { tier: 'Heuristic', conf: 0.5, who: 'synthesizers and your TOML rules' },
  { tier: 'Tags', conf: 0.3, who: 'a tag-scan guess' },
]

function Provenance() {
  return (
    <Section id="provenance">
      <Head title="Edges grep never sees, each with its confidence." sub="A rule you write in TOML connects an event to its consumers. Every edge says how sure it is and where it came from." />
      <div className="grid lg:grid-cols-2 gap-4 items-start">
        <div className="rock-panel p-0">
          <div className="px-5 py-3 border-b border-hairline-strong text-sm text-muted font-sans">An event bus, made visible</div>
          <div className="px-5 py-4">
            <svg viewBox="0 0 420 110" className="w-full h-auto" role="img" aria-label="checkout emits order.paid; sendReceipt and updateLedger consume it">
              <rect x="6" y="40" width="110" height="30" rx="8" className="q-node" /><text x="61" y="59" textAnchor="middle" className="q-node-t">checkout()</text>
              <rect x="160" y="40" width="100" height="30" rx="8" className="q-node q-node--center" /><text x="210" y="59" textAnchor="middle" className="q-node-t q-node-t--center">order.paid</text>
              <rect x="304" y="10" width="110" height="30" rx="8" className="q-node" /><text x="359" y="29" textAnchor="middle" className="q-node-t">sendReceipt()</text>
              <rect x="304" y="70" width="110" height="30" rx="8" className="q-node" /><text x="359" y="89" textAnchor="middle" className="q-node-t">updateLedger()</text>
              <line x1="116" y1="55" x2="160" y2="55" className="q-edge q-edge--inj" />
              <line x1="304" y1="25" x2="260" y2="50" className="q-edge q-edge--inj" />
              <line x1="304" y1="85" x2="260" y2="60" className="q-edge q-edge--inj" />
            </svg>
            <pre className="toml"><code>{`[[rule]]
name      = "event-bus-consume"
file_glob = "**/*.js"
pattern   = "subscribe\\\\([\\"'](?P<topic>[\\\\w.]+)"`}</code></pre>
            <p className="mt-3 text-sm text-muted font-sans">A new kind of edge is a rule block, not a code change. These land at 0.5, labelled with the rule’s name.</p>
          </div>
        </div>
        <div className="rock-panel p-0">
          <div className="px-5 py-3 border-b border-hairline-strong text-sm text-muted font-sans">How sure each edge is</div>
          <table className="tiers">
            <tbody>
              {TIERS.map(t => (
                <tr key={t.tier}>
                  <th scope="row">{t.tier}</th>
                  <td className="tiers-bar"><span style={{ width: `${t.conf * 100}%` }} /></td>
                  <td className="tiers-n">{t.conf.toFixed(1)}</td>
                  <td className="tiers-who">{t.who}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="px-5 pb-4 text-sm text-muted font-sans">When two tiers report the same edge, the higher one wins. A 0.3 guess is never handed to an agent as a 1.0 fact.</p>
        </div>
      </div>
    </Section>
  )
}

// ── 4 · Everything an agent can call — an explorer ──────────────────────────
// MCP: crates/wicked-estate-mcp/src/lib.rs + src/tools/{memory,knowledge,proposals}.rs.
// CLI: crates/wicked-estate/src/main.rs (FEATURES.md §10).
type Row = { name: string; does: string }
type Group = { label?: string; rows: Row[] }
const TOOLSETS: { id: string; name: string; note: string; groups: Group[] }[] = [
  { id: 'graph', name: 'Graph', note: '12 MCP tools, +1 with embeddings', groups: [
    { label: 'Find', rows: [
      { name: 'SearchEntity', does: 'Find symbols by name or kind' },
      { name: 'RetrieveEntity', does: 'One symbol, with its edges and notes' },
      { name: 'FetchContent', does: 'The source of a symbol or file' },
      { name: 'SemanticSearch', does: 'Search by meaning (when embeddings exist)' },
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
  { id: 'memory', name: 'Memory', note: '7 tools + 4 for proposals', groups: [
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
  { id: 'knowledge', name: 'Knowledge', note: '7 tools', groups: [
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
  { id: 'cli', name: 'CLI', note: 'same record, from your shell', groups: [
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

function Toolface() {
  const [set, setSet] = useState('graph')
  const [filter, setFilter] = useState('')
  const f = filter.trim().toLowerCase()
  const shown = f
    ? TOOLSETS.map(t => ({ ...t, groups: t.groups.map(g => ({ ...g, rows: g.rows.filter(r => (r.name + ' ' + r.does).toLowerCase().includes(f)) })).filter(g => g.rows.length) })).filter(t => t.groups.length)
    : TOOLSETS.filter(t => t.id === set)
  const total = (t: typeof TOOLSETS[number]) => t.groups.reduce((n, g) => n + g.rows.length, 0)
  return (
    <Section id="toolface" solid>
      <Head title="Everything an agent can call." sub="30 MCP tools in three groups, and a CLI over the same record. Pick a group, or search them all." />
      <div className="tools" data-set={f ? 'search' : set}>
        <div className="tools-bar">
          <div className="tools-tabs" role="tablist" aria-label="Tool groups">
            {TOOLSETS.map(t => (
              <button key={t.id} type="button" role="tab" aria-selected={!f && set === t.id} data-on={!f && set === t.id} className="tools-tab"
                onClick={() => { setSet(t.id); setFilter('') }}>
                {t.name} <span className="q-count">{total(t)}</span>
              </button>
            ))}
          </div>
          <input className="tools-search" type="search" placeholder="Search tools, e.g. erase" aria-label="Search tools" value={filter} onChange={e => setFilter(e.target.value)} />
        </div>
        {shown.length === 0 && <p className="px-5 py-6 text-muted font-sans">No tool matches “{filter}”.</p>}
        {shown.map(t => (
          <div key={t.id} className="tools-set">
            <p className="tools-note">{f ? t.name : t.note}</p>
            <div className="tools-groups">
              {t.groups.map(g => (
                <div key={g.label} className="tools-group">
                  <h3 className="tools-gl">{g.label}</h3>
                  <dl>
                    {g.rows.map(r => (
                      <div key={r.name} className="tools-row">
                        <dt><code>{r.name}</code></dt>
                        <dd>{r.does}</dd>
                      </div>
                    ))}
                  </dl>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </Section>
  )
}

// ── 5 · Solo or shared ──────────────────────────────────────────────────────
function Storage() {
  const [shared, setShared] = useState(false)
  const rows: [string, string, string][] = [
    ['Store', 'one graph.db file per repo', 'a Postgres you host'],
    ['Writers', 'one writer, no daemon', 'the whole team, concurrently'],
    ['Setup', 'nothing to run', 'WICKED_RUNTIME=team and WICKED_STORE_URL, on a --features postgres build'],
    ['MCP server', 'yes', 'not yet; it refuses to start rather than fall back'],
  ]
  return (
    <Section id="storage">
      <Head title="SQLite by default. Postgres for a team." sub="Same schema and the same engine either way. Nothing leaves your machine unless you point it at a shared store." />
      <div className="inline-flex flex-wrap gap-1.5 p-1.5 rounded-xl mb-4" style={{ background: 'var(--rock)', border: '1px solid var(--hairline-strong)' }}>
        <button className="seg" data-on={!shared} onClick={() => setShared(false)}>SQLite · solo</button>
        <button className="seg" data-on={shared} onClick={() => setShared(true)}>PostgreSQL · team</button>
      </div>
      <div className="rock-panel p-0 max-w-3xl">
        <div className="px-5 py-3 border-b border-hairline-strong font-mono text-sm text-ink" data-storage-cmd>
          {shared ? 'WICKED_RUNTIME=team wicked-estate index .' : 'wicked-estate index . --db graph.db'}
        </div>
        <table className="store-t">
          <tbody>
            {rows.map(([k, solo, team]) => (
              <tr key={k}><th scope="row">{k}</th><td>{shared ? team : solo}</td></tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  )
}

// ── 6 · Get started (its own island, after the SameGarden map) ──────────────
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
  { comment: '# the CLI and the MCP server', cmd: 'cargo install wicked-estate wicked-estate-mcp' },
  { comment: '# index your repo', cmd: 'wicked-estate index . --db graph.db' },
  { comment: '# connect your agent (Claude Code shown)',
    cmd: 'claude mcp add wicked-estate -s project -- wicked-estate-mcp --db "$PWD/graph.db"',
    show: 'claude mcp add wicked-estate -s project \\\n    -- wicked-estate-mcp --db "$PWD/graph.db"' },
]

export function GetStarted() {
  return (
    <Section id="get-started" solid>
      <Head title="Index a repo and ask it something." sub="Use an absolute database path: agent CLIs start from unpredictable directories. Works with Claude Code, Antigravity, Codex, OpenCode and Pi." />
      <div className="grid md:grid-cols-2 gap-4">
        <div className="rock-panel p-0">
          <div className="px-5 py-3 border-b border-hairline-strong flex items-center justify-between">
            <span className="text-sm font-sans text-ink">Recommended: the whole family</span>
            <span className="depth">npm</span>
          </div>
          <div className="px-5 py-5">
            <div className="flex items-center gap-3">
              <div className="font-mono text-sm text-ink flex-1 min-w-0"><span style={{ color: 'var(--accent)' }}>$ </span>{INSTALL_CMD}</div>
              <CopyBtn text={INSTALL_CMD} label={INSTALL_CMD} />
            </div>
            <p className="mt-3 text-sm text-muted font-sans leading-snug">Pick estate and any siblings, choose your agent CLIs, and it wires them up.</p>
          </div>
        </div>
        <div className="rock-panel p-0">
          <div className="px-5 py-3 border-b border-hairline-strong flex items-center justify-between">
            <span className="text-sm font-sans text-ink">Or just estate</span>
            <span className="depth">crates.io</span>
          </div>
          <div className="px-5 py-5 font-mono text-xs leading-[1.9]">
            {DIRECT_CMDS.map(c => (
              <div key={c.cmd} className="mb-2 last:mb-0">
                <div className="text-faint">{c.comment}</div>
                <div className="flex items-start gap-3">
                  <div className="text-ink flex-1 min-w-0 whitespace-pre-wrap"><span style={{ color: 'var(--accent)' }}>$ </span>{c.show ?? c.cmd}</div>
                  <CopyBtn text={c.cmd} label={c.cmd} />
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
      <div className="mt-7 flex flex-col sm:flex-row gap-3">
        <a href="https://github.com/mikeparcewski/wicked-estate" target="_blank" rel="noreferrer" className="btn-primary"><GitHubIcon /> GitHub</a>
        <a href="https://github.com/mikeparcewski/wicked-estate/tree/main/docs" target="_blank" rel="noreferrer" className="btn-outline">Documentation</a>
      </div>
    </Section>
  )
}

// The SameGarden map (an Astro component) renders between this island and GetStarted.
export default function Content() {
  return (
    <>
      <Hero />
      <Stores />
      <Provenance />
      <Toolface />
      <Storage />
    </>
  )
}
