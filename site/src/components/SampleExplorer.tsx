"use client";

import { useRef, useState } from "react";
import CodeBlock from "@/components/CodeBlock";
import { SAMPLES } from "@/content/samples";

/*
 * A tablist over the fixture samples.
 *
 * Follows the APG tabs pattern with automatic activation: arrows move focus
 * and selection together, Home/End jump to the ends, and only the selected tab
 * is in the tab sequence.
 */
export default function SampleExplorer() {
  const [selected, setSelected] = useState(0);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const select = (index: number) => {
    const next = (index + SAMPLES.length) % SAMPLES.length;
    setSelected(next);
    tabRefs.current[next]?.focus();
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        event.preventDefault();
        select(selected + 1);
        break;
      case "ArrowLeft":
      case "ArrowUp":
        event.preventDefault();
        select(selected - 1);
        break;
      case "Home":
        event.preventDefault();
        select(0);
        break;
      case "End":
        event.preventDefault();
        select(SAMPLES.length - 1);
        break;
      default:
        break;
    }
  };

  const sample = SAMPLES[selected];

  return (
    <div>
      <div
        role="tablist"
        aria-label="Code samples"
        onKeyDown={onKeyDown}
        className="border-hairline flex flex-wrap border-b"
      >
        {SAMPLES.map((item, index) => {
          const active = index === selected;
          return (
            <button
              key={item.id}
              ref={(node) => {
                tabRefs.current[index] = node;
              }}
              type="button"
              role="tab"
              id={`sample-tab-${item.id}`}
              aria-selected={active}
              aria-controls={`sample-panel-${item.id}`}
              tabIndex={active ? 0 : -1}
              onClick={() => setSelected(index)}
              className={`panel-hover -mb-px border-b-2 px-5 py-3 text-[13px] font-bold tracking-[0.1em] uppercase ${
                active
                  ? "border-cinnabar text-text"
                  : "text-secondary hover:text-text border-transparent"
              }`}
            >
              {item.label}
            </button>
          );
        })}
      </div>

      <div
        role="tabpanel"
        id={`sample-panel-${sample.id}`}
        aria-labelledby={`sample-tab-${sample.id}`}
        tabIndex={0}
        data-testid="sample-panel"
        className="pt-8 focus-visible:outline-offset-4"
      >
        <p className="text-secondary mb-7 max-w-[70ch] text-[16px] leading-[1.7] text-pretty">
          {sample.summary}
        </p>
        <CodeBlock
          code={sample.code}
          linearHandles={sample.linearHandles}
          caption={sample.source}
        />
      </div>
    </div>
  );
}
