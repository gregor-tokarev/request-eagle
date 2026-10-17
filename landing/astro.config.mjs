import { defineConfig } from "astro/config";
import { satteri } from "@astrojs/markdown-satteri";
import sitemap from "@astrojs/sitemap";

// The guides link each other as `name.md`, which GitHub resolves. On the site they are pages under /docs/.
const guideLinks = {
  name: "guide-links",
  link(node, context) {
    const match = /^([a-z-]+)\.md(#.*)?$/.exec(node.url);

    if (match) context.setProperty(node, "url", `/docs/${match[1]}/${match[2] ?? ""}`);
  },
};

export default defineConfig({
  site: "https://requesteagle.tokarev.work",

  // The sitemap at /sitemap-index.xml tells search engines about every page; robots.txt points to it.
  integrations: [sitemap()],

  // The stylesheets are small, so they travel inside the page instead of blocking it with a request.
  build: { inlineStylesheets: "always" },

  markdown: {
    processor: satteri({ mdastPlugins: [guideLinks] }),
    shikiConfig: {
      theme: "night-owl",
      // The theme colours the tokens; the page styles the block itself, like the landing's own code.
      transformers: [{ pre(node) { delete node.properties.style; } }],
    },
  },
});
