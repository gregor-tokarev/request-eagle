import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://requesteagle.tokarev.work",

  // The stylesheets are small, so they travel inside the page instead of blocking it with a request.
  build: { inlineStylesheets: "always" },
});
