const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const finePointer = window.matchMedia("(hover: hover) and (pointer: fine)").matches;

const clamp = (value, min, max) => Math.min(Math.max(value, min), max);

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
const band = document.querySelector("[data-band]");
const layers = document.querySelectorAll("[data-parallax]");

// 0 when the element's top reaches the bottom of the viewport, 1 when its bottom leaves the top.
function progress(element) {
  const bounds = element.getBoundingClientRect();

  return clamp((window.innerHeight - bounds.top) / (window.innerHeight + bounds.height), 0, 1);
}

function followScroll() {
  nav.classList.toggle("is-scrolled", window.scrollY > 24);

  if (reducedMotion) return;

  // The window lies back at first and stands up as it rises into view.
  const upright = clamp(progress(stage) * 2.2 - 0.25, 0, 1);
  const frame = stage.firstElementChild;

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

if (finePointer && !reducedMotion) {
  for (const element of document.querySelectorAll("[data-tilt]")) {
    element.addEventListener("pointermove", (event) => {
      const bounds = element.getBoundingClientRect();
      const x = (event.clientX - bounds.left) / bounds.width;
      const y = (event.clientY - bounds.top) / bounds.height;

      element.classList.add("is-tilting");
      element.style.setProperty("--ry", `${((x - 0.5) * 7).toFixed(2)}deg`);
      element.style.setProperty("--rx", `${((0.5 - y) * 7).toFixed(2)}deg`);
      element.style.setProperty("--gx", `${(x * 100).toFixed(1)}%`);
      element.style.setProperty("--gy", `${(y * 100).toFixed(1)}%`);
    });

    element.addEventListener("pointerleave", () => {
      element.classList.remove("is-tilting");
      element.style.setProperty("--rx", "0deg");
      element.style.setProperty("--ry", "0deg");
    });
  }

  for (const button of document.querySelectorAll("[data-magnetic]")) {
    button.addEventListener("pointermove", (event) => {
      const bounds = button.getBoundingClientRect();

      button.style.setProperty("--mx", `${((event.clientX - bounds.left - bounds.width / 2) * 0.18).toFixed(1)}px`);
      button.style.setProperty("--my", `${((event.clientY - bounds.top - bounds.height / 2) * 0.3).toFixed(1)}px`);
    });

    button.addEventListener("pointerleave", () => {
      button.style.setProperty("--mx", "0px");
      button.style.setProperty("--my", "0px");
    });
  }
}

// Takeoff: the icon faces the pointer, and the streaks accelerate near the buttons.

const takeoff = document.querySelector("[data-takeoff]");

if (!reducedMotion) {
  const icon = takeoff.querySelector("[data-icon]");
  const boost = takeoff.querySelector("[data-boost]");
  const streaks = takeoff.querySelector(".streaks").getAnimations({ subtree: true });

  const setSpeed = (rate) => streaks.forEach((animation) => animation.updatePlaybackRate(rate));

  if (finePointer) {
    takeoff.addEventListener("pointermove", (event) => {
      const bounds = icon.getBoundingClientRect();
      const x = clamp((event.clientX - bounds.left - bounds.width / 2) / (window.innerWidth / 2), -1, 1);
      const y = clamp((event.clientY - bounds.top - bounds.height / 2) / (window.innerHeight / 2), -1, 1);

      icon.style.setProperty("--ry", `${(x * 22).toFixed(2)}deg`);
      icon.style.setProperty("--rx", `${(y * -22).toFixed(2)}deg`);
    });

    takeoff.addEventListener("pointerleave", () => {
      icon.style.setProperty("--rx", "0deg");
      icon.style.setProperty("--ry", "0deg");
    });
  }

  boost.addEventListener("pointerenter", () => setSpeed(3.5));
  boost.addEventListener("pointerleave", () => setSpeed(1));
  boost.addEventListener("focusin", () => setSpeed(3.5));
  boost.addEventListener("focusout", () => setSpeed(1));
}

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

if (!reducedMotion) stageWatcher.observe(themeStage);

picker.addEventListener("pointerenter", endRotation);
picker.addEventListener("focusin", endRotation);

for (const button of choices) {
  button.addEventListener("click", () => {
    endRotation();
    showTheme(button);
  });
}
