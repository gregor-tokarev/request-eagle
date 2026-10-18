import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://requesteagle.tokarev.work",

  // The sitemap at /sitemap-index.xml tells search engines about every page; robots.txt points to it.
  integrations: [sitemap()],

  // The stylesheets are small, so they travel inside the page instead of blocking it with a request.
  build: { inlineStylesheets: "always" },
});
