import React from "react";
import {
  parseReleaseMarkdown,
  type InlineToken,
} from "../utils/releaseMarkdown";

const renderInlineTokens = (
  tokens: InlineToken[],
  keyPrefix: string
): React.ReactNode[] =>
  tokens.map((tok, idx) => {
    const key = `${keyPrefix}-${idx}`;
    switch (tok.type) {
      case "text":
        return <React.Fragment key={key}>{tok.value}</React.Fragment>;
      case "code":
        return (
          <code key={key} className="release-notes-code">
            {tok.value}
          </code>
        );
      case "bold":
        return (
          <strong key={key} className="release-notes-strong">
            {renderInlineTokens(tok.children, key)}
          </strong>
        );
      case "italic":
        return (
          <em key={key} className="release-notes-em">
            {renderInlineTokens(tok.children, key)}
          </em>
        );
      case "link":
        return (
          <span key={key} className="release-notes-link" title={tok.url}>
            {tok.label}
          </span>
        );
    }
  });

type ReleaseNotesViewProps = {
  body?: string | null;
  emptyText?: string;
};

export const ReleaseNotesView: React.FC<ReleaseNotesViewProps> = ({
  body,
  emptyText,
}) => {
  const trimmed = body?.trim() ?? "";
  if (!trimmed) {
    return emptyText ? (
      <div className="release-notes">
        <p className="release-notes-empty">{emptyText}</p>
      </div>
    ) : null;
  }

  const blocks = parseReleaseMarkdown(trimmed);
  if (blocks.length === 0) {
    return (
      <div className="release-notes">
        <p className="release-notes-paragraph">{trimmed}</p>
      </div>
    );
  }

  return (
    <div className="release-notes">
      {blocks.map((block, idx) => {
        const key = `blk-${idx}`;
        switch (block.type) {
          case "heading":
            return (
              <div
                key={key}
                className={`release-notes-heading level-${Math.min(block.level, 4)}`}
              >
                {renderInlineTokens(block.tokens, key)}
              </div>
            );
          case "ul":
            return (
              <ul key={key} className="release-notes-list">
                {block.items.map((item, itemIdx) => (
                  <li
                    key={`${key}-${itemIdx}`}
                    className={item.nested ? "nested" : undefined}
                  >
                    {renderInlineTokens(item.tokens, `${key}-${itemIdx}`)}
                  </li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={key} className="release-notes-ordered">
                {block.items.map((item, itemIdx) => (
                  <li
                    key={`${key}-${itemIdx}`}
                    className={item.nested ? "nested" : undefined}
                  >
                    {renderInlineTokens(item.tokens, `${key}-${itemIdx}`)}
                  </li>
                ))}
              </ol>
            );
          case "quote":
            return (
              <blockquote key={key} className="release-notes-quote">
                {block.lines.map((lineTokens, lineIdx) => (
                  <div key={`${key}-q-${lineIdx}`}>
                    {renderInlineTokens(lineTokens, `${key}-q-${lineIdx}`)}
                  </div>
                ))}
              </blockquote>
            );
          case "paragraph":
            return (
              <p key={key} className="release-notes-paragraph">
                {renderInlineTokens(block.tokens, key)}
              </p>
            );
        }
      })}
    </div>
  );
};
