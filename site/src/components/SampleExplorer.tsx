"use client";

import { useRef, useState } from "react";
import CodeBlock from "@/components/CodeBlock";
import { SAMPLES } from "@/content/samples";
import { ICON } from "@/lib/constants";

const DISPLAY_SAMPLES = [
  ...SAMPLES.filter((sample) => sample.id === "vec"),
  ...SAMPLES.filter((sample) => sample.id !== "vec"),
];

/*
 * A tablist over the fixture samples.
 *
 * Follows the APG tabs pattern with automatic activation: arrows move focus
 * and selection together, Home/End jump to the ends, and only the selected tab
 * is in the tab sequence.
 */
export default function SampleExplorer({
  summaries,
}: {
  /**
   * What each sample shows, keyed by sample id. Authored in the home route's
   * content.md and passed down, because a client component cannot read a file.
   */
  summaries: Record<string, string>;
}) {
  const [selected, setSelected] = useState(0);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const select = (index: number) => {
    const next = (index + DISPLAY_SAMPLES.length) % DISPLAY_SAMPLES.length;
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
        select(DISPLAY_SAMPLES.length - 1);
        break;
      default:
        break;
    }
  };

  const sample = DISPLAY_SAMPLES[selected];

  return (
    <div>
      <div
        role="tablist"
        aria-label="Code samples"
        onKeyDown={onKeyDown}
        className="border-hairline flex flex-wrap border-b"
      >
        {DISPLAY_SAMPLES.map((item, index) => {
          const active = index === selected;
          const Icon = item.icon;
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
              className={`panel-hover -mb-px inline-flex items-center gap-2.5 border-b-2 px-5 py-3 text-[13px] font-bold tracking-widest uppercase ${
                active
                  ? "border-cinnabar text-text"
                  : "text-secondary hover:text-text hover:border-hairline-strong border-transparent"
              }`}
            >
              {/*
                The same lock-up an Action uses: the plate 07 figure at the
                16px step, then the label. Decorative — the label is the
                accessible name, and repeating it in the icon would make every
                tab announce itself twice.
              */}
              <Icon size={ICON.inline} />
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
          {summaries[sample.id]}
        </p>
        <CodeBlock
          code={sample.code}
          linearHandles={sample.linearHandles}
          path={sample.source}
          title={sample.label}
        />
      </div>
    </div>
  );
}
