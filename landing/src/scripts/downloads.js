// Styles keep the navigation bar opaque until this script is running to track the scroll.
document.documentElement.classList.add("js");

const LATEST = "https://api.github.com/repos/gregor-tokarev/request-eagle/releases/latest";

const nav = document.querySelector("[data-nav]");
const streaks = document.querySelector(".streaks");
const boost = document.querySelector("[data-boost]");
const image = document.querySelector("[data-image]");
const archive = document.querySelector("[data-archive]");

function followScroll() {
  nav.classList.toggle("is-scrolled", window.scrollY > 24);
}

window.addEventListener("scroll", followScroll, { passive: true });
followScroll();

// The streaks accelerate near the buttons. They have no animations while motion is reduced.
function setStreakSpeed(rate) {
  streaks.getAnimations({ subtree: true }).forEach((animation) => animation.updatePlaybackRate(rate));
}

boost.addEventListener("pointerenter", () => setStreakSpeed(3.5));
boost.addEventListener("pointerleave", () => setStreakSpeed(1));
boost.addEventListener("focusin", () => setStreakSpeed(3.5));
boost.addEventListener("focusout", () => setStreakSpeed(1));

// The newest release is read from GitHub on each visit, so publishing one is all it takes to update this page.

async function loadLatest() {
  const response = await fetch(LATEST, { headers: { Accept: "application/vnd.github+json" } });

  if (!response.ok) throw new Error(`GitHub answered ${response.status}`);

  return response.json();
}

// A file missing from the release leaves its button pointing at GitHub.
function offer(release) {
  const file = (ending) => release.assets.find((asset) => asset.name.endsWith(ending));

  const files = { image: file(".dmg"), archive: file(".zip") };

  if (files.image) image.href = files.image.browser_download_url;
  if (files.archive) archive.href = files.archive.browser_download_url;
}

loadLatest().then(offer).catch(console.error);
