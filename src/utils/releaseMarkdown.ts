export type InlineToken =
  | { type: "text"; value: string }
  | { type: "code"; value: string }
  | { type: "bold"; children: InlineToken[] }
  | { type: "italic"; children: InlineToken[] }
  | { type: "link"; label: string; url: string };

export type MarkdownListItem = {
  tokens: InlineToken[];
  nested: boolean;
};

export type MarkdownBlock =
  | { type: "heading"; level: number; tokens: InlineToken[] }
  | { type: "ul"; items: MarkdownListItem[] }
  | { type: "ol"; items: MarkdownListItem[] }
  | { type: "quote"; lines: InlineToken[][] }
  | { type: "paragraph"; tokens: InlineToken[] };

const DELIMITERS = [
  { open: "**", close: "**", type: "bold" as const, requireBoundary: false },
  { open: "__", close: "__", type: "bold" as const, requireBoundary: true },
  { open: "*", close: "*", type: "italic" as const, requireBoundary: true },
  { open: "_", close: "_", type: "italic" as const, requireBoundary: true },
];

const isWordChar = (ch: string | undefined): boolean =>
  ch !== undefined && /[\p{L}\p{N}]/u.test(ch);

/** Tokenizes inline GitHub Markdown (`code`, `**bold**`, `*italic*`, `[label](url)`) without raw syntax artifacts. */
export function tokenizeInlineMarkdown(input: string): InlineToken[] {
  const tokens: InlineToken[] = [];
  let cursor = 0;

  const pushText = (slice: string) => {
    if (!slice) return;
    const prev = tokens[tokens.length - 1];
    if (prev && prev.type === "text") {
      prev.value += slice;
    } else {
      tokens.push({ type: "text", value: slice });
    }
  };

  while (cursor < input.length) {
    if (input[cursor] === "`") {
      const end = input.indexOf("`", cursor + 1);
      if (end > cursor + 1) {
        tokens.push({ type: "code", value: input.slice(cursor + 1, end) });
        cursor = end + 1;
        continue;
      }
    }

    if (input[cursor] === "[") {
      const linkMatch = input
        .slice(cursor)
        .match(/^\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/);
      if (linkMatch) {
        tokens.push({
          type: "link",
          label: linkMatch[1],
          url: linkMatch[2],
        });
        cursor += linkMatch[0].length;
        continue;
      }
    }

    let matchedDelim = false;
    for (const delim of DELIMITERS) {
      if (!input.startsWith(delim.open, cursor)) continue;
      if (delim.requireBoundary && isWordChar(input[cursor - 1])) continue;
      const start = cursor + delim.open.length;
      if (input[start] === " " || input[start] === "\t") continue;
      const end = input.indexOf(delim.close, start);
      if (end <= start) continue;
      if (input[end - 1] === " " || input[end - 1] === "\t") continue;
      const afterClose = input[end + delim.close.length];
      if (delim.requireBoundary && isWordChar(afterClose)) continue;

      const inner = input.slice(start, end);
      tokens.push({
        type: delim.type,
        children: tokenizeInlineMarkdown(inner),
      });
      cursor = end + delim.close.length;
      matchedDelim = true;
      break;
    }
    if (matchedDelim) continue;

    pushText(input[cursor]);
    cursor += 1;
  }

  return tokens;
}

/** Parses GitHub Release Markdown into structured blocks so headings, lists, and highlights render cleanly. */
export function parseReleaseMarkdown(markdown: string): MarkdownBlock[] {
  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  const blocks: MarkdownBlock[] = [];
  let i = 0;

  const isHorizontalRule = (line: string) => /^[-*_]{3,}\s*$/.test(line.trim());
  const isHeading = (line: string) => /^(#{1,6})\s+(.+)$/.test(line.trim());
  const matchBullet = (line: string) => line.match(/^(\s*)[-*+]\s+(.+)$/);
  const matchOrdered = (line: string) => line.match(/^(\s*)\d+[.)]\s+(.+)$/);
  const isQuote = (line: string) => line.trimStart().startsWith(">");

  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    if (!trimmed || isHorizontalRule(line)) {
      i += 1;
      continue;
    }

    const headingMatch = trimmed.match(/^(#{1,6})\s+(.+)$/);
    if (headingMatch) {
      blocks.push({
        type: "heading",
        level: headingMatch[1].length,
        tokens: tokenizeInlineMarkdown(headingMatch[2].trim()),
      });
      i += 1;
      continue;
    }

    if (matchBullet(line)) {
      const items: MarkdownListItem[] = [];
      while (i < lines.length) {
        const m = matchBullet(lines[i]);
        if (!m) break;
        items.push({
          nested: m[1].length >= 2,
          tokens: tokenizeInlineMarkdown(m[2].trim()),
        });
        i += 1;
      }
      blocks.push({ type: "ul", items });
      continue;
    }

    if (matchOrdered(line)) {
      const items: MarkdownListItem[] = [];
      while (i < lines.length) {
        const m = matchOrdered(lines[i]);
        if (!m) break;
        items.push({
          nested: m[1].length >= 2,
          tokens: tokenizeInlineMarkdown(m[2].trim()),
        });
        i += 1;
      }
      blocks.push({ type: "ol", items });
      continue;
    }

    if (isQuote(line)) {
      const quoteLines: InlineToken[][] = [];
      while (i < lines.length && isQuote(lines[i])) {
        const cleaned = lines[i].trimStart().replace(/^>\s?/, "").trim();
        if (cleaned) {
          quoteLines.push(tokenizeInlineMarkdown(cleaned));
        }
        i += 1;
      }
      if (quoteLines.length > 0) {
        blocks.push({ type: "quote", lines: quoteLines });
      }
      continue;
    }

    const paraLines: string[] = [];
    while (i < lines.length) {
      const candidate = lines[i];
      if (
        !candidate.trim() ||
        isHorizontalRule(candidate) ||
        isHeading(candidate) ||
        matchBullet(candidate) ||
        matchOrdered(candidate) ||
        isQuote(candidate)
      ) {
        break;
      }
      paraLines.push(candidate.trim());
      i += 1;
    }
    if (paraLines.length > 0) {
      blocks.push({
        type: "paragraph",
        tokens: tokenizeInlineMarkdown(paraLines.join(" ")),
      });
    }
  }

  return blocks;
}
