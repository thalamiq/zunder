import React, { useState, useMemo, useCallback, memo } from "react";
import { Braces, ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "@thalamiq/ui/utils";
import { detectValueType } from "@/lib/json";

interface JsonViewerProps {
  data: unknown;
  copyable?: boolean;
  downloadable?: boolean;
  theme?: "light" | "dark" | "auto";
  maxHeight?: string;
  className?: string;
  onDataChange?: (data: unknown) => void;
  maxSizeForTreeView?: number; // Max size in bytes before fallback to simple view
  maxSizeForHighlighting?: number; // Max size in bytes before disabling highlighting
}

// Memoized value renderer
const JsonValue = memo(({ value }: { value: unknown }) => {
  if (value === null) {
    return <span className="json-null">null</span>;
  }

  if (typeof value === "boolean") {
    return <span className="json-boolean">{value.toString()}</span>;
  }

  if (typeof value === "number") {
    return <span className="json-number">{value}</span>;
  }

  if (typeof value === "string") {
    const type = detectValueType(value);

    if (type === "date") {
      return (
        <span className="json-date">
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
          <span style={{ userSelect: "text" }}>{value}</span>
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
        </span>
      );
    }

    if (type === "url") {
      return (
        <span className="json-url">
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
          <span style={{ userSelect: "text" }}>{value}</span>
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
        </span>
      );
    }

    if (type === "reference") {
      return (
        <span className="json-reference">
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
          <span style={{ userSelect: "text" }}>{value}</span>
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
        </span>
      );
    }

    if (type === "uuid") {
      return (
        <span className="json-uuid">
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
          <span style={{ userSelect: "text" }}>{value}</span>
          <span className="json-quote" style={{ userSelect: "none" }}>
            &quot;
          </span>
        </span>
      );
    }

    return (
      <span className="json-string">
        <span className="json-quote" style={{ userSelect: "none" }}>
          &quot;
        </span>
        <span style={{ userSelect: "text" }}>{value}</span>
        <span className="json-quote" style={{ userSelect: "none" }}>
          &quot;
        </span>
      </span>
    );
  }

  return null;
});
JsonValue.displayName = "JsonValue";

// Memoized JSON node renderer
interface JsonNodeProps {
  data: unknown;
  keyName?: string;
  path: string;
  isLast: boolean;
  collapsedPaths: Set<string>;
  togglePath: (path: string) => void;
}

const JsonNode = memo(
  ({
    data,
    keyName,
    path,
    isLast,
    collapsedPaths,
    togglePath,
  }: JsonNodeProps) => {
    const isArray = Array.isArray(data);
    const isObject = data !== null && typeof data === "object" && !isArray;
    const isCollapsible = isArray || isObject;
    const isCollapsed = collapsedPaths.has(path);

    if (!isCollapsible) {
      return (
        <div className="json-line">
          {keyName && (
            <>
              <span className="json-key">&quot;{keyName}&quot;</span>
              <span className="json-separator">: </span>
            </>
          )}
          <span className="json-value-wrapper">
            <JsonValue value={data} />
          </span>
          {!isLast && <span className="json-separator">,</span>}
        </div>
      );
    }

    const entries = isArray
      ? data.map((item, idx) => [idx.toString(), item])
      : Object.entries(data);

    const handleToggle = (e: React.MouseEvent) => {
      e.stopPropagation();
      togglePath(path);
    };

    return (
      <div className="json-node">
        <div className="json-line">
          {isCollapsible && (
            <button
              onClick={handleToggle}
              className="json-toggle"
              aria-label={isCollapsed ? "Expand" : "Collapse"}
            >
            {isCollapsed ? (
              <ChevronRight className="w-2.5 h-2.5" />
            ) : (
              <ChevronDown className="w-2.5 h-2.5" />
            )}
            </button>
          )}
          {keyName && (
            <>
              <span className="json-key">&quot;{keyName}&quot;</span>
              <span className="json-separator">: </span>
            </>
          )}
          <span>{isArray ? "[" : "{"}</span>
          {isCollapsed && (
            <>
              <span className="json-collapsed">
                {" "}
                ... {entries.length} {isArray ? "items" : "properties"}{" "}
              </span>
              <span>{isArray ? "]" : "}"}</span>
              {!isLast && <span>,</span>}
            </>
          )}
        </div>

        {!isCollapsed && (
          <>
            <div className="json-children">
              {entries.map(([key, value], idx) => (
                <JsonNode
                  key={`${path}.${key}`}
                  data={value}
                  keyName={isArray ? undefined : key}
                  path={`${path}.${key}`}
                  isLast={idx === entries.length - 1}
                  collapsedPaths={collapsedPaths}
                  togglePath={togglePath}
                />
              ))}
            </div>
            <div className="json-line">
              <span>{isArray ? "]" : "}"}</span>
              {!isLast && <span className="json-separator">,</span>}
            </div>
          </>
        )}
      </div>
    );
  }
);
JsonNode.displayName = "JsonNode";

const JsonViewer = ({
  data,
  className,
  maxSizeForTreeView = 1000000, // Default 1MB
  maxSizeForHighlighting = 5000000, // Default 5MB
}: JsonViewerProps) => {
  // Memos
  const jsonString = useMemo(() => JSON.stringify(data, null, 2), [data]);

  const dataSize = useMemo(() => new Blob([jsonString]).size, [jsonString]);

  // Determine if data is too large for tree view
  const isTooLargeForTreeView = useMemo(
    () => dataSize > maxSizeForTreeView,
    [dataSize, maxSizeForTreeView]
  );

  // Determine if data is too large for highlighting
  const isTooLargeForHighlighting = useMemo(
    () => dataSize > maxSizeForHighlighting,
    [dataSize, maxSizeForHighlighting]
  );

  // State for line numbers and tree view
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(new Set());

  // Toggle collapse state for a path
  const togglePath = useCallback((path: string) => {
    setCollapsedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  // FHIR-aware JSON syntax highlighting with clickable links
  const highlightedJson = useMemo(() => {
    let result = jsonString;

    // Patterns for FHIR-specific values
    const patterns = {
      // ISO 8601 date/datetime
      date: /^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?)?$/,
      // URLs and URIs (http, https, urn)
      url: /^(https?:\/\/|urn:)/,
      // FHIR references (ResourceType/id or just id)
      reference: /^[A-Z][a-zA-Z]+\/[a-zA-Z0-9\-\.]+$/,
      // UUIDs
      uuid: /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
    };

    // Match and highlight key-value pairs
    result = result.replace(
      /("(?:[^"\\]|\\.)*")\s*:\s*("(?:[^"\\]|\\.)*"|true|false|null|-?\d+\.?\d*(?:[eE][+-]?\d+)?|\[|\{)/g,
      (_match, key, value) => {
        let highlightedValue = value;

        // Highlight based on value type
        if (value.startsWith('"')) {
          const stringValue = value.slice(1, -1); // Remove quotes
          const escapedStringValue = stringValue.replace(/"/g, "&quot;");

          // Check for FHIR-specific patterns
          if (patterns.date.test(stringValue)) {
            highlightedValue = `<span class="json-date"><span class="json-quote" style="user-select: none;">&quot;</span><span style="user-select: text;">${escapedStringValue}</span><span class="json-quote" style="user-select: none;">&quot;</span></span>`;
          } else if (patterns.url.test(stringValue)) {
            highlightedValue = `<span class="json-url"><span class="json-quote" style="user-select: none;">&quot;</span><span style="user-select: text;">${escapedStringValue}</span><span class="json-quote" style="user-select: none;">&quot;</span></span>`;
          } else if (patterns.reference.test(stringValue)) {
            highlightedValue = `<span class="json-reference"><span class="json-quote" style="user-select: none;">&quot;</span><span style="user-select: text;">${escapedStringValue}</span><span class="json-quote" style="user-select: none;">&quot;</span></span>`;
          } else if (patterns.uuid.test(stringValue)) {
            highlightedValue = `<span class="json-uuid"><span class="json-quote" style="user-select: none;">&quot;</span><span style="user-select: text;">${escapedStringValue}</span><span class="json-quote" style="user-select: none;">&quot;</span></span>`;
          } else {
            highlightedValue = `<span class="json-string"><span class="json-quote" style="user-select: none;">&quot;</span><span style="user-select: text;">${escapedStringValue}</span><span class="json-quote" style="user-select: none;">&quot;</span></span>`;
          }
        } else if (value === "true" || value === "false") {
          highlightedValue = `<span class="json-boolean">${value}</span>`;
        } else if (value === "null") {
          highlightedValue = `<span class="json-null">${value}</span>`;
        } else if (
          !isNaN(parseFloat(value)) &&
          value !== "[" &&
          value !== "{"
        ) {
          highlightedValue = `<span class="json-number">${value}</span>`;
        }

        return `<span class="json-key">${key}</span>: ${highlightedValue}`;
      }
    );

    return result;
  }, [jsonString]);

  if (!data) {
    return (
      <div className="flex items-center justify-center py-6 text-muted-foreground text-[11px]">
        <span className="flex items-center gap-1.5">
          <Braces className="w-3.5 h-3.5 opacity-60" />
          No data
        </span>
      </div>
    );
  }

  return (
    <div className={cn("max-w-full overflow-x-auto", className)}>
      {/* Content */}
      {!isTooLargeForTreeView ? (
        <div className="px-3 py-2 text-[11px] font-mono text-foreground json-tree-view overflow-x-auto">
          <JsonNode
            data={data}
            path="root"
            isLast={true}
            collapsedPaths={collapsedPaths}
            togglePath={togglePath}
          />
        </div>
      ) : isTooLargeForHighlighting ? (
        <div className="flex overflow-x-auto">
          <pre className="flex-1 text-[11px] font-mono text-foreground px-3 py-2 whitespace-pre-wrap wrap-break-word leading-[1.35] max-w-full">
            <code className="json-syntax wrap-break-word">{jsonString}</code>
          </pre>
        </div>
      ) : (
        <div className="flex overflow-x-auto">
          <pre className="flex-1 text-[11px] font-mono text-foreground px-3 py-2 whitespace-pre-wrap wrap-break-word leading-[1.35] max-w-full">
            <code
              dangerouslySetInnerHTML={{ __html: highlightedJson }}
              className="json-syntax wrap-break-word"
            />
          </pre>
        </div>
      )}

      {/* Inline styles — subtle, enterprise palette */}
      <style>{`
        .json-tree-view {
          line-height: 1.35;
          overflow-wrap: break-word;
          word-break: break-word;
          max-width: 100%;
        }

        .json-node {
          display: block;
          max-width: 100%;
          overflow-wrap: break-word;
        }

        .json-line {
          display: flex;
          align-items: flex-start;
          flex-wrap: wrap;
          min-height: 1.35em;
          gap: 0.125rem;
        }

        .json-value-wrapper {
          display: inline;
          margin: 0;
          padding: 0;
          word-break: break-word;
          overflow-wrap: break-word;
          max-width: 100%;
        }

        .json-separator {
          user-select: none;
          -webkit-user-select: none;
          -moz-user-select: none;
          -ms-user-select: none;
        }

        .json-children {
          padding-left: 1rem;
          border-left: 1px solid hsl(var(--border) / 0.6);
          margin-left: 0.375rem;
          max-width: 100%;
          overflow-wrap: break-word;
        }

        .json-toggle {
          display: inline-flex;
          align-items: center;
          justify-content: center;
          padding: 0;
          margin: 0;
          background: none;
          border: none;
          cursor: pointer;
          color: hsl(var(--muted-foreground) / 0.8);
          flex-shrink: 0;
        }

        .json-toggle:hover {
          color: hsl(var(--muted-foreground));
        }

        .json-collapsed {
          color: hsl(var(--muted-foreground) / 0.9);
          font-style: italic;
          font-size: 0.9em;
        }

        .json-date, .json-string, .json-url, .json-reference, .json-uuid,
        .json-number, .json-boolean, .json-null {
          white-space: normal;
          display: inline-block;
          word-break: break-word;
          overflow-wrap: break-word;
          max-width: 100%;
        }

        .json-date > span, .json-string > span, .json-url > span,
        .json-reference > span, .json-uuid > span {
          display: inline;
          word-break: break-word;
          overflow-wrap: break-word;
        }

        .json-date > span[style*="user-select: text"],
        .json-string > span[style*="user-select: text"],
        .json-url > span[style*="user-select: text"],
        .json-reference > span[style*="user-select: text"],
        .json-uuid > span[style*="user-select: text"] {
          white-space: pre-wrap;
          word-break: break-word;
          overflow-wrap: break-word;
        }
      `}</style>
    </div>
  );
};

export default memo(JsonViewer);
