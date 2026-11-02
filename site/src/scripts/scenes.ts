/* Scene behaviour for the estate page (site-scoped; the chrome is untouched).

   1. In-view marks: each scene gets data-inview the first time a fair share of it is on
      screen, once. Scene CSS keys its one-shot entrance off it (html.motion-ok only), and
      .scenes-ready is set only after the observer is attached, so a page whose script never
      runs shows every end state instead of hiding anything.
   2. The scene rail: aria-current on the dot of the scene under the middle of the viewport.
   3. Guarded snap: html.snap-lock (scroll-snap-type: y mandatory, global.css) only while the
      viewport is a desktop one (>=1024 x >=640, motion allowed) AND every scene measures
      inside its frame. Otherwise the page stays on the chrome's proximity snap, so a scene
      that ever ran tall can never trap content. */

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
    { threshold: 0.3 },
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
function frameOf(el: HTMLElement) {
  const bar = parseFloat(getComputedStyle(html).getPropertyValue('--topbar-h')) || 64;
  return el.classList.contains('strata--hero') ? window.innerHeight : window.innerHeight - bar;
}
let snapFrame = 0;
function checkSnap() {
  snapFrame = 0;
  const fits = desk.matches && scenes.every((s) => s.getBoundingClientRect().height <= frameOf(s) + 1);
  html.classList.toggle('snap-lock', fits);
}
const queue = () => { if (!snapFrame) snapFrame = requestAnimationFrame(checkSnap); };
if ('ResizeObserver' in window) {
  const ro = new ResizeObserver(queue);
  for (const s of scenes) ro.observe(s);
}
window.addEventListener('resize', queue);
desk.addEventListener?.('change', queue);
queue();
