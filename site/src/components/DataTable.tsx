import type { ReactNode } from "react";
import type { Row } from "@/content/cli";

/*
 * A table in the board's hairline grid.
 *
 * Two shapes, because the reference page needs both and they were previously
 * written out twice:
 *
 * - `rows`: the common name/description pair, where the name is a row header.
 * - `headings` + `data`: an arbitrary grid, where the first cell of each row
 *   is still the row header.
 *
 * Either way the first column is a `<th scope="row">`, not a `<td>`: these
 * tables are lookups, and the flag or command names the row.
 */

const HEAD_CELL =
  "border-hairline text-label border-b px-5 py-3 font-mono text-[10px] font-medium tracking-[0.16em] uppercase";
const ROW_HEADER =
  "border-hairline text-text border-b px-5 py-3.5 text-left align-top font-mono text-[13px] font-normal whitespace-nowrap";
const CELL =
  "border-hairline text-secondary border-b px-5 py-3.5 align-top text-[14.5px] leading-relaxed";

type DataTableProps =
  | {
      rows: readonly Row[];
      nameHeading: string;
      headings?: never;
      data?: never;
      className?: string;
    }
  | {
      headings: readonly string[];
      data: readonly (readonly ReactNode[])[];
      rows?: never;
      nameHeading?: never;
      className?: string;
    };

export default function DataTable(props: DataTableProps) {
  const headings = props.rows
    ? [props.nameHeading, "What it does"]
    : props.headings;
  const data = props.rows
    ? props.rows.map((row) => [row.name, row.description] as const)
    : props.data;

  return (
    <div className={`rule-grid mt-8 block overflow-x-auto ${props.className ?? ""}`}>
      <table className="bg-ground w-full border-collapse text-left">
        <thead className="bg-panel">
          <tr>
            {headings.map((heading, index) => (
              <th
                key={heading}
                scope="col"
                className={`${HEAD_CELL}${index === 0 ? " whitespace-nowrap" : ""}`}
              >
                {heading}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.map((cells, rowIndex) => (
            <tr key={rowIndex}>
              <th scope="row" className={ROW_HEADER}>
                {cells[0]}
              </th>
              {cells.slice(1).map((cell, cellIndex) => (
                <td
                  key={cellIndex}
                  className={`${CELL}${props.rows ? "" : " whitespace-nowrap"}`}
                >
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
