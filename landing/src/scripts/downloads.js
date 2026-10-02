// Styles keep the navigation bar opaque until this script is running to track the scroll.
document.documentElement.classList.add("js");

const RELEASES = "https://api.github.com/repos/gregor-tokarev/request-eagle/releases?per_page=30";

const nav = document.querySelector("[data-nav]");
const streaks = document.querySelector(".streaks");
const boost = document.querySelector("[data-boost]");
const trackButtons = document.querySelectorAll("[data-track]");
const note = document.querySelector("[data-track-note]");
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

// Releases are read from GitHub on each visit, so publishing or promoting one is all it takes to update this page.

async function loadReleases() {
  const response = await fetch(RELEASES, { headers: { Accept: "application/vnd.github+json" } });

  if (!response.ok) throw new Error(`GitHub answered ${response.status}`);

  return response.json();
}

const releases = loadReleases();

// Stable releases are the ones that are not prereleases. Nightly builds include them, since a stable
// release was a nightly build first. GitHub lists the newest release first.
async function offer(track) {
  const candidates = (await releases).filter((release) => track === "nightly" || !release.prerelease);

  for (const link of files) {
    const asset = candidates
      .flatMap((release) => release.assets)
      .find((candidate) => candidate.name.endsWith(link.dataset.file));

    // A platform without a file on this track leads to the list of releases on GitHub.
    link.href = asset ? asset.browser_download_url : "https://github.com/gregor-tokarev/request-eagle/releases";
  }
}

// The address remembers the track, so a link can lead straight to the nightly builds.
function choose(track) {
  trackButtons.forEach((button) => button.setAttribute("aria-pressed", String(button.dataset.track === track)));
  note.textContent = note.dataset[track];
  history.replaceState(null, "", track === "nightly" ? "#nightly" : location.pathname);

  offer(track).catch(console.error);
}

trackButtons.forEach((button) => button.addEventListener("click", () => choose(button.dataset.track)));

choose(location.hash === "#nightly" ? "nightly" : "stable");
