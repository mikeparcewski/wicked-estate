/* Scene behaviour for the estate page (site-scoped; the chrome is untouched).

   1. In-view marks: each scene gets data-inview the first time it rises into the upper two
      thirds of the viewport, once. Scene CSS keys its one-shot entrance off it (html.motion-ok only), and
      .scenes-ready is set only after the observer is attached, so a page whose script never
      runs shows every end state instead of hiding anything.
   2. (The beat track and the thread under the topbar are src/scripts/beats.js, the same as wicked-crew's.)
   2b. Live marks: data-live on each scene while a good part of it is on screen. The calm idle loops
      (global.css) key off it, so they run only in view and stop when the scene leaves.
   3. Guarded snap, desktop: html.snap-lock (scroll-snap-type: y mandatory, global.css) only while the
      viewport is a desktop one (>=1024 x >=640, motion allowed) AND every scene measures
      inside its frame, give or take its own bottom padding (an overshoot smaller than the padding hides
      no content). Otherwise the page does not snap at all (not the chrome's proximity: with scenes one
      frame tall, proximity pulls each notch back to the scene it left), so nothing can trap the reader.
   4. One wheel gesture = one scene, while snap-lock holds. Under mandatory snap Chromium pulls a
      small wheel step back to the scene it started from, so a notched mouse could never leave the hero.
      The page is one frame per scene there, so nothing sits between two snap points: the wheel pages.
      A gesture is a run of wheel events in one direction with no gap over GAP_MS: a mouse notch, a
      trackpad swipe and its momentum. Its first event moves one scene in its direction, whatever the
      device or the size of the delta; the rest of the gesture is absorbed. A gesture that starts while
      a glide is still running chains from that glide's target, so two notches always move two scenes
      and a reversal comes back. A scroller inside a scene keeps its own wheel until it reaches its end,
      and the gesture that scrolled it stays its own: only a new gesture at the end pages the scene.
      ctrl+wheel (zoom) is never touched.
   Phones scroll freely (the family chrome's rule: snapping on a phone yanks the viewport). A scene there may
   run to two or three screens (the hero, the confidence ladder, storage, the family map); each of its
   frames (.ph-frame, global.css) fits one screen, and nothing locks, so a tall scene never traps. */

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

// 2b · live marks (the idle loops run only while a scene is on screen)
if ('IntersectionObserver' in window) {
  const live = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        const el = e.target as HTMLElement;
        // "in view" = at least 40% of the viewport (or of the scene, when it is shorter than that) shows it
        const need = Math.min(e.boundingClientRect.height, window.innerHeight) * 0.4;
        if (e.isIntersecting && e.intersectionRect.height >= need) el.dataset.live = '1';
        else delete el.dataset.live;
      }
    },
    { threshold: [0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8, 1] },
  );
  for (const s of scenes) live.observe(s);
}

// 3 · guarded mandatory snap
const desk = matchMedia('(min-width: 1024px) and (min-height: 640px) and (prefers-reduced-motion: no-preference)');
const bar = () => parseFloat(getComputedStyle(html).getPropertyValue('--topbar-h')) || 64;
function frameOf(el: HTMLElement) {
  return el.classList.contains('strata--hero') ? window.innerHeight : window.innerHeight - bar();
}
/** How far a scene may run past its frame without hiding content: its own bottom padding, less 2px. */
function slack(el: HTMLElement) {
  return Math.max(1, (parseFloat(getComputedStyle(el).paddingBottom) || 0) - 2);
}
let snapFrame = 0;
function checkSnap() {
  snapFrame = 0;
  html.classList.toggle('snap-lock', desk.matches && scenes.every((s) => s.getBoundingClientRect().height <= frameOf(s) + slack(s)));
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
// reduced motion switched on mid-glide: the browser's smooth scroll would run on, so stop it where it is
const reduce = matchMedia('(prefers-reduced-motion: reduce)');
reduce.addEventListener?.('change', () => {
  if (!reduce.matches) return;
  dropGlide();
  // the lock goes now, not on the guard's next frame, so the stop below is not snapped somewhere else
  html.classList.remove('snap-lock');
  // a real move cancels the browser's running smooth scroll (a scroll to where it already is may not)
  const y = window.scrollY;
  window.scrollTo({ top: y + 1, behavior: 'instant' });
  window.scrollTo({ top: y, behavior: 'instant' });
});
window.addEventListener('wheel', (e) => {
  if (!html.classList.contains('snap-lock') || e.ctrlKey || e.defaultPrevented) return;
  if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
  const now = performance.now();
  const dir = e.deltaY > 0 ? 1 : -1;
  const fresh = now - lastAt > GAP_MS || dir !== lastDir;
  // the gesture is recorded before an inner scroller takes it: a swipe that scrolls the tools list to its
  // end, and that swipe's momentum, stay one gesture, so the page never flips in the same gesture
  lastAt = now;
  lastDir = dir;
  if (innerScrolls(e.target, e.deltaY)) return;
  e.preventDefault();
  if (!fresh) return; // the rest of a gesture: more of a swipe, its momentum, a notch's own repeats
  const from = glideTo !== null && now < glideUntil ? glideTo : window.scrollY;
  const tops = sceneTops();
  const to = dir > 0 ? tops.find((t) => t > from + 4) : [...tops].reverse().find((t) => t < from - 4);
  if (to === undefined) return;
  glideTo = to;
  glideUntil = now + GLIDE_MS;
  window.scrollTo({ top: to, behavior: 'smooth' });
}, { passive: false });
