// Styles keep the navigation bar opaque until this script is running to track the scroll.
document.documentElement.classList.add("js");

const LATEST = "https://api.github.com/repos/gregor-tokarev/request-eagle/releases/latest";

const nav = document.querySelector("[data-nav]");
const streaks = document.querySelector(".streaks");
const boost = document.querySelector("[data-boost]");
const files = document.querySelectorAll("[data-file]");

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

// The latest release is read from GitHub on each visit, so promoting one is all it takes to update this page.
// It is the newest stable release; nightly builds are chosen in the app's settings, not here.

async function loadLatest() {
  const response = await fetch(LATEST, { headers: { Accept: "application/vnd.github+json" } });

  if (!response.ok) throw new Error(`GitHub answered ${response.status}`);

  return response.json();
}

// A platform without a file in the release keeps its button on the release's page on GitHub.
function offer(release) {
  for (const link of files) {
    const asset = release.assets.find((candidate) => candidate.name.endsWith(link.dataset.file));

    if (asset) link.href = asset.browser_download_url;
  }
}

loadLatest().then(offer).catch(console.error);
