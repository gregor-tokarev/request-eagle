// Styles keep the navigation bar opaque until this script is running to track the scroll.
document.documentElement.classList.add("js");

const API = "https://api.github.com/repos/gregor-tokarev/request-eagle/releases";

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

const responses = {};

// Each address is fetched once per visit, when a track first needs it.
function load(address) {
  responses[address] ??= fetch(address, { headers: { Accept: "application/vnd.github+json" } }).then((response) => {
    if (!response.ok) throw new Error(`GitHub answered ${response.status}`);

    return response.json();
  });

  return responses[address];
}

// The recent releases, newest first, are mostly nightly builds, and include the stable ones among
// them, since each was a nightly build first. The latest release is the newest stable one, which
// may be older than all of them.
async function releasesOn(track) {
  const recent = await load(`${API}?per_page=30`);

  if (track === "nightly") return recent;

  return [await load(`${API}/latest`), ...recent.filter((release) => !release.prerelease)];
}

let chosen;

async function offer(track) {
  const candidates = await releasesOn(track);

  // A slower lookup for the track chosen before must not replace these links.
  if (track !== chosen) return;

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
  chosen = track;
  trackButtons.forEach((button) => button.setAttribute("aria-pressed", String(button.dataset.track === track)));
  note.textContent = note.dataset[track];
  history.replaceState(null, "", track === "nightly" ? "#nightly" : location.pathname);

  offer(track).catch(console.error);
}

trackButtons.forEach((button) => button.addEventListener("click", () => choose(button.dataset.track)));

choose(location.hash === "#nightly" ? "nightly" : "stable");
