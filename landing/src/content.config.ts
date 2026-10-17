import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";

// The guides stay in the repository's docs/ folder, where contributors edit them and GitHub shows them.
// The site publishes these six as pages under /docs/.
const docs = defineCollection({
  loader: glob({
    base: "../docs",
    pattern: ["scripting.md", "collection-runner.md", "flows.md", "git.md", "agent.md", "cli.md"],
  }),
});

export const collections = { docs };
