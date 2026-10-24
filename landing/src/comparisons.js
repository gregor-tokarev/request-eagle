// The API clients the site compares Request Eagle with, in the order the pages list them.
// Each has a page at /compare/<slug>/; the hub at /compare/ lists them all.
export const comparisons = [
  {
    slug: "postman",
    name: "Postman",
    blurb: "The most used API client: an Electron app built around a cloud workspace and an account.",
  },
  {
    slug: "insomnia",
    name: "Insomnia",
    blurb: "Kong's Electron client with a design-first workflow, cloud, Git and local storage options.",
  },
  {
    slug: "bruno",
    name: "Bruno",
    blurb: "An open-source Electron client that keeps collections as plain-text files in a folder.",
  },
  {
    slug: "yaak",
    name: "Yaak",
    blurb: "A Tauri client from Insomnia's original author, with a Rust backend and a web view.",
  },
  {
    slug: "hoppscotch",
    name: "Hoppscotch",
    blurb: "An open-source client that runs in the browser, with a desktop app and a self-hosted edition.",
  },
];

// The month the facts on the comparison pages were last checked, and the Request Eagle they were checked with.
export const checked = "October 2026";
export const eagleVersion = "0.1.41";
