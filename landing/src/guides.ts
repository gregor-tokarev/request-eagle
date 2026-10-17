// The guides in the order the docs index lists them.
export const order = ["scripting", "collection-runner", "flows", "git", "agent", "cli"];

// A guide's title is its first heading.
export function title(body: string): string {
  return body.split("\n", 1)[0].replace(/^#\s*/, "").trim();
}

// A guide's summary is its first paragraph, as plain text.
export function summary(body: string): string {
  const paragraph = body.split(/\n\s*\n/)[1] ?? "";

  return paragraph
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/[*`]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}
