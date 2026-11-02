/* Scene behaviour for the estate page (site-scoped; the chrome is untouched).

   1. In-view marks: each scene gets data-inview the first time it rises into the upper two
      thirds of the viewport, once. Scene CSS keys its one-shot entrance off it (html.motion-ok only), and
      .scenes-ready is set only after the observer is attached, so a page whose script never
      runs shows every end state instead of hiding anything.
   2. The scene rail: aria-current on the dot of the scene under the middle of the viewport.
   3. Guarded snap, desktop: html.snap-lock (scroll-snap-type: y mandatory, global.css) only while the
      viewport is a desktop one (>=1024 x >=640, motion allowed) AND every scene measures
      inside its frame. Otherwise the page stays on the chrome's proximity snap, so a scene
      that ever ran tall can never trap content.
   4. One wheel gesture = one scene, while snap-lock holds. Under mandatory snap Chromium pulls a
      small wheel step back to the scene it started from, so a notched mouse could never leave the hero.
      The page is one frame per scene there, so nothing sits between two snap points: the wheel pages.
      A gesture is a run of wheel events in one direction with no gap over GAP_MS: a mouse notch, a
      trackpad swipe and its momentum. Its first event moves one scene in its direction, whatever the
      device or the size of the delta; the rest of the gesture is absorbed. A gesture that starts while
      a glide is still running chains from that glide's target, so two notches always move two scenes
      and a reversal comes back. A scroller inside a scene keeps its own wheel until it reaches its end,
      and ctrl+wheel (zoom) is never touched.
   Phones scroll freely (the family chrome's rule: snapping on a phone yanks the viewport); their
   frames are laid out to fit one screen each (global.css), with nothing to lock. */

const html = document.documentElement;
const scenes = Array.from(document.querySelectorAll<HTMLElement>('main [data-scene], main .same-garden'));

// 1 · one-shot entrance marks
if ('IntersectionObserver' in window) {
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) {
          (e.target as HTMLElement).dataset.inview = '1';
          io.unobserve(e.target);
        }
      }
    },
    // a line (the scene's top rising past two thirds of the viewport), not a visible share: a share can be
    // unreachable for a scene taller than a short or zoomed window, which would leave its pre-state showing
    { threshold: 0, rootMargin: '0px 0px -33% 0px' },
  );
  for (const s of scenes) io.observe(s);
  html.classList.add('scenes-ready');
}

// 2 · the rail
const rail = Array.from(document.querySelectorAll<HTMLAnchorElement>('.scene-rail a[data-rail]'));
const targets = rail.map((a) => document.getElementById(a.dataset.rail ?? '')).filter((t): t is HTMLElement => !!t);
let railFrame = 0;
function markRail() {
  railFrame = 0;
  const mid = window.innerHeight * 0.5;
  let current = targets[0];
  for (const t of targets) {
    if (t.getBoundingClientRect().top <= mid) current = t;
  }
  for (const a of rail) {
    if (a.dataset.rail === current?.id) a.setAttribute('aria-current', 'true');
    else a.removeAttribute('aria-current');
  }
}
window.addEventListener('scroll', () => { if (!railFrame) railFrame = requestAnimationFrame(markRail); }, { passive: true });
markRail();

// 3 · guarded mandatory snap
const desk = matchMedia('(min-width: 1024px) and (min-height: 640px) and (prefers-reduced-motion: no-preference)');
const bar = () => parseFloat(getComputedStyle(html).getPropertyValue('--topbar-h')) || 64;
function frameOf(el: HTMLElement) {
  return el.classList.contains('strata--hero') ? window.innerHeight : window.innerHeight - bar();
}
let snapFrame = 0;
function checkSnap() {
  snapFrame = 0;
  html.classList.toggle('snap-lock', desk.matches && scenes.every((s) => s.getBoundingClientRect().height <= frameOf(s) + 1));
}
const queue = () => { if (!snapFrame) snapFrame = requestAnimationFrame(checkSnap); };
if ('ResizeObserver' in window) {
  const ro = new ResizeObserver(queue);
  for (const s of scenes) ro.observe(s);
}
window.addEventListener('resize', queue);
desk.addEventListener?.('change', queue);
queue();

// 4 · one wheel notch = one scene (desktop, while snap-lock holds)
function sceneTops() {
  const max = document.documentElement.scrollHeight - window.innerHeight;
  const els = [...scenes, ...Array.from(document.querySelectorAll<HTMLElement>('footer.footer'))];
  const tops = els.map((el, i) => Math.round(Math.min(max, Math.max(0, el.getBoundingClientRect().top + window.scrollY - (i === 0 ? 0 : bar())))));
  return [...new Set(tops)].sort((a, b) => a - b);
}
/** A scroller between the wheel's target and the page that can still move in this direction keeps the wheel. */
function innerScrolls(target: EventTarget | null, dy: number) {
  for (let el = target instanceof Element ? target : null; el && el !== document.body && el !== html; el = el.parentElement) {
    const oy = getComputedStyle(el).overflowY;
    if ((oy === 'auto' || oy === 'scroll') && el.scrollHeight > el.clientHeight + 1) {
      if (dy > 0 ? el.scrollTop + el.clientHeight < el.scrollHeight - 1 : el.scrollTop > 0) return true;
    }
  }
  return false;
}
const GAP_MS = 140;   // a pause this long ends a gesture (mouse notches at a reading pace are further apart)
const GLIDE_MS = 700; // how long a glide may run; a gesture starting inside it chains from its target
let lastAt = -Infinity;
let lastDir = 0;
let glideTo: number | null = null;
let glideUntil = -Infinity;
// a glide's target counts only while that glide runs: its end, or any other way of moving (keys, a click or
// tap, focus moving, an anchor) drops it, and the next notch starts from where the page really is
const dropGlide = () => { glideTo = null; };
for (const ev of ['scrollend', 'keydown', 'pointerdown', 'focusin', 'hashchange']) window.addEventListener(ev, dropGlide, { passive: true });
window.addEventListener('wheel', (e) => {
  if (!html.classList.contains('snap-lock') || e.ctrlKey || e.defaultPrevented) return;
  if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
  if (innerScrolls(e.target, e.deltaY)) return;
  e.preventDefault();
  const now = performance.now();
  const dir = e.deltaY > 0 ? 1 : -1;
  const fresh = now - lastAt > GAP_MS || dir !== lastDir;
  lastAt = now;
  lastDir = dir;
  if (!fresh) return; // the rest of a gesture: more of a swipe, its momentum, a notch's own repeats
  const from = glideTo !== null && now < glideUntil ? glideTo : window.scrollY;
  const tops = sceneTops();
  const to = dir > 0 ? tops.find((t) => t > from + 4) : [...tops].reverse().find((t) => t < from - 4);
  if (to === undefined) return;
  glideTo = to;
  glideUntil = now + GLIDE_MS;
  window.scrollTo({ top: to, behavior: 'smooth' });
}, { passive: false });
