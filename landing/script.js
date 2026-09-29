// Styles only hide content for reveal once this script is running to show it again.
document.documentElement.classList.add("js");

// Read when motion is about to happen, so a preference changed mid-visit takes effect at once.
const motionPreference = window.matchMedia("(prefers-reduced-motion: reduce)");
const motionAllowed = () => !motionPreference.matches;

const finePointer = window.matchMedia("(hover: hover) and (pointer: fine)").matches;

const clamp = (value, min, max) => Math.min(Math.max(value, min), max);

function clearProperties(element, ...names) {
  names.forEach((name) => element.style.removeProperty(name));
}

// Reveal on scroll

const revealed = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;

      entry.target.classList.add("is-visible");
      revealed.unobserve(entry.target);
    }
  },
  { rootMargin: "0px 0px -12% 0px", threshold: 0.12 },
);

document.querySelectorAll("[data-reveal]").forEach((element) => revealed.observe(element));

// Scroll-linked motion: the navigation bar, the hero window, the band and the layered captures.

const nav = document.querySelector("[data-nav]");
const stage = document.querySelector("[data-stage]");
const frame = stage.firstElementChild;
const band = document.querySelector("[data-band]");
const layers = document.querySelectorAll("[data-parallax]");

// 0 when the element's top reaches the bottom of the viewport, 1 when its bottom leaves the top.
function progress(element) {
  const bounds = element.getBoundingClientRect();

  return clamp((window.innerHeight - bounds.top) / (window.innerHeight + bounds.height), 0, 1);
}

function followScroll() {
  nav.classList.toggle("is-scrolled", window.scrollY > 24);

  if (!motionAllowed()) return;

  // The window lies back at first and stands up as it rises into view.
  const upright = clamp(progress(stage) * 2.2 - 0.25, 0, 1);

  frame.style.setProperty("--tilt", ((1 - upright) * 16).toFixed(2));
  frame.style.setProperty("--scale", (0.92 + upright * 0.08).toFixed(3));

  band.style.setProperty("--shift", progress(band).toFixed(4));

  for (const layer of layers) {
    layer.style.setProperty("--p", progress(layer).toFixed(4));
  }
}

let scrollQueued = false;

function queueScroll() {
  if (scrollQueued) return;

  scrollQueued = true;

  requestAnimationFrame(() => {
    scrollQueued = false;
    followScroll();
  });
}

window.addEventListener("scroll", queueScroll, { passive: true });
window.addEventListener("resize", queueScroll);
followScroll();

// Pointer-driven motion is limited to devices that hover.

const tilting = document.querySelectorAll("[data-tilt]");
const magnetic = document.querySelectorAll("[data-magnetic]");

function releaseTilt(element) {
  element.classList.remove("is-tilting");
  clearProperties(element, "--rx", "--ry");
}

if (finePointer) {
  for (const element of tilting) {
    element.addEventListener("pointermove", (event) => {
      if (!motionAllowed()) return;

      const bounds = element.getBoundingClientRect();
      const x = (event.clientX - bounds.left) / bounds.width;
      const y = (event.clientY - bounds.top) / bounds.height;

      element.classList.add("is-tilting");
      element.style.setProperty("--ry", `${((x - 0.5) * 7).toFixed(2)}deg`);
      element.style.setProperty("--rx", `${((0.5 - y) * 7).toFixed(2)}deg`);
      element.style.setProperty("--gx", `${(x * 100).toFixed(1)}%`);
      element.style.setProperty("--gy", `${(y * 100).toFixed(1)}%`);
    });

    element.addEventListener("pointerleave", () => releaseTilt(element));
  }

  for (const button of magnetic) {
    button.addEventListener("pointermove", (event) => {
      if (!motionAllowed()) return;

      const bounds = button.getBoundingClientRect();

      button.style.setProperty("--mx", `${((event.clientX - bounds.left - bounds.width / 2) * 0.18).toFixed(1)}px`);
      button.style.setProperty("--my", `${((event.clientY - bounds.top - bounds.height / 2) * 0.3).toFixed(1)}px`);
    });

    button.addEventListener("pointerleave", () => clearProperties(button, "--mx", "--my"));
  }
}

// Takeoff: the icon faces the pointer, and the streaks accelerate near the buttons.

const takeoff = document.querySelector("[data-takeoff]");
const icon = takeoff.querySelector("[data-icon]");
const boost = takeoff.querySelector("[data-boost]");

// Looked up on each use: the streaks have no animations while motion is reduced.
function setStreakSpeed(rate) {
  const streaks = takeoff.querySelector(".streaks").getAnimations({ subtree: true });

  streaks.forEach((animation) => animation.updatePlaybackRate(rate));
}

if (finePointer) {
  takeoff.addEventListener("pointermove", (event) => {
    if (!motionAllowed()) return;

    const bounds = icon.getBoundingClientRect();
    const x = clamp((event.clientX - bounds.left - bounds.width / 2) / (window.innerWidth / 2), -1, 1);
    const y = clamp((event.clientY - bounds.top - bounds.height / 2) / (window.innerHeight / 2), -1, 1);

    icon.style.setProperty("--ry", `${(x * 22).toFixed(2)}deg`);
    icon.style.setProperty("--rx", `${(y * -22).toFixed(2)}deg`);
  });

  takeoff.addEventListener("pointerleave", () => clearProperties(icon, "--rx", "--ry"));
}

boost.addEventListener("pointerenter", () => setStreakSpeed(3.5));
boost.addEventListener("pointerleave", () => setStreakSpeed(1));
boost.addEventListener("focusin", () => setStreakSpeed(3.5));
boost.addEventListener("focusout", () => setStreakSpeed(1));

// Race: each lane fills in the time its app took to start, so the wait is felt rather than read.

const race = document.querySelector("[data-race]");
const again = race.querySelector("[data-again]");

const lanes = [...race.querySelectorAll("[data-lane]")].map((element) => ({
  element,
  fill: element.querySelector("[data-fill]"),
  time: element.querySelector("[data-time]"),
  duration: Number(element.dataset.ms),
}));

const seconds = (milliseconds) => `${(milliseconds / 1000).toFixed(2)} s`;

function showLane(lane, elapsed) {
  const shown = Math.min(elapsed, lane.duration);

  lane.fill.style.transform = `scaleX(${shown / lane.duration})`;
  lane.time.textContent = seconds(shown);
  lane.element.classList.toggle("is-ready", shown === lane.duration);
}

let raceFrame = 0;

function runRace() {
  cancelAnimationFrame(raceFrame);

  const started = performance.now();

  const step = (now) => {
    const elapsed = now - started;

    lanes.forEach((lane) => showLane(lane, elapsed));

    if (lanes.some((lane) => elapsed < lane.duration)) raceFrame = requestAnimationFrame(step);
  };

  raceFrame = requestAnimationFrame(step);
}

function finishRace() {
  cancelAnimationFrame(raceFrame);
  lanes.forEach((lane) => showLane(lane, lane.duration));
}

const raceWatcher = new IntersectionObserver(([entry]) => {
  if (!entry.isIntersecting) return;

  raceWatcher.disconnect();

  if (motionAllowed()) runRace();
}, { threshold: 0.6 });

raceWatcher.observe(race.querySelector(".race__lanes"));
again.addEventListener("click", runRace);

// The lanes wait at the start until the race scrolls into view.
if (motionAllowed()) lanes.forEach((lane) => showLane(lane, 0));

again.hidden = !motionAllowed();

// Theme picker: the chosen capture fades in over the current one, then replaces it.

const picker = document.querySelector("[data-picker]");
const themeStage = document.querySelector("[data-theme-stage]");
const [current, next] = themeStage.querySelectorAll("img");
const choices = [...picker.querySelectorAll("button")];

const FADE = 450;
const ROTATE_EVERY = 2800;

let latestChoice = 0;

async function showTheme(button) {
  const choice = ++latestChoice;

  for (const other of choices) {
    other.setAttribute("aria-pressed", String(other === button));
  }

  next.src = button.dataset.src;
  await next.decode().catch(() => {});

  // A newer choice was made while this capture was loading.
  if (choice !== latestChoice) return;

  next.classList.add("is-shown");
  await new Promise((resolve) => setTimeout(resolve, FADE));

  if (choice !== latestChoice) return;

  current.src = button.dataset.src;
  current.alt = `Request Eagle in the ${button.textContent.trim()} theme.`;
  await current.decode().catch(() => {});

  if (choice !== latestChoice) return;

  next.classList.remove("is-shown");
}

// The captures rotate once through while the stage is on screen. Reaching for the
// picker ends the rotation, so its buttons never change under a pointer or focus.
let rotation = 0;
let rotationsLeft = choices.length;

function pauseRotation() {
  clearInterval(rotation);
  rotation = 0;
}

function endRotation() {
  stageWatcher.disconnect();
  pauseRotation();
}

const stageWatcher = new IntersectionObserver(([entry]) => {
  pauseRotation();

  if (!entry.isIntersecting) return;

  rotation = setInterval(() => {
    const pressed = choices.findIndex((button) => button.getAttribute("aria-pressed") === "true");

    showTheme(choices[(pressed + 1) % choices.length]);

    if (--rotationsLeft === 0) endRotation();
  }, ROTATE_EVERY);
}, { threshold: 0.4 });

if (motionAllowed()) stageWatcher.observe(themeStage);

picker.addEventListener("pointerenter", endRotation);
picker.addEventListener("focusin", endRotation);

for (const button of choices) {
  button.addEventListener("click", () => {
    endRotation();
    showTheme(button);
  });
}

// A preference changed mid-visit: bring everything in motion to rest, or let it move again.

motionPreference.addEventListener("change", () => {
  again.hidden = !motionAllowed();

  if (motionAllowed()) {
    followScroll();

    return;
  }

  finishRace();
  endRotation();

  clearProperties(frame, "--tilt", "--scale");
  clearProperties(band, "--shift");
  layers.forEach((layer) => clearProperties(layer, "--p"));

  tilting.forEach(releaseTilt);
  magnetic.forEach((button) => clearProperties(button, "--mx", "--my"));
  clearProperties(icon, "--rx", "--ry");
});
