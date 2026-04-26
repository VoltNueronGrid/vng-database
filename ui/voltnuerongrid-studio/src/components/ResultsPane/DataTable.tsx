import { useState } from "react";
import type { ResultColumn } from "@/store/query";

interface DataTableProps {
  columns: ResultColumn[];
  rows: Array<Record<string, unknown>>;
}

function cellClass(value: unknown, colType: string): string {
  if (value === null || value === undefined) return "cell-null";
  const t = colType.toUpperCase();
  if (t.includes("INT") || t.includes("FLOAT") || t.includes("DECIMAL") || t.includes("NUM"))
    return "cell-num";
  if (t.includes("BOOL")) return value ? "cell-bool-t" : "cell-bool-f";
  if (t.includes("DATE") || t.includes("TIME")) return "cell-date";
  return "";
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

type SortDir = "asc" | "desc" | null;

export function DataTable({ columns, rows }: DataTableProps) {
  const [sortCol, setSortCol] = useState<string | null>(null);
  const [sortDir, setSortDir] = useState<SortDir>(null);
  const [selectedRow, setSelectedRow] = useState<number | null>(null);

  function handleSort(col: string) {
    if (sortCol === col) {
      setSortDir((d) => (d === "asc" ? "desc" : d === "desc" ? null : "asc"));
      if (sortDir === "desc") setSortCol(null);
    } else {
      setSortCol(col);
      setSortDir("asc");
    }
  }

  const sorted = [...rows].sort((a, b) => {
    if (!sortCol || !sortDir) return 0;
    const av = a[sortCol];
    const bv = b[sortCol];
    const cmp = String(av ?? "").localeCompare(String(bv ?? ""), undefined, {
      numeric: true,
    });
    return sortDir === "asc" ? cmp : -cmp;
  });

  return (
    <div className="data-table-wrap">
      <table className="data-table">
        <thead>
          <tr>
            <th className="row-num">#</th>
            {columns.map((col) => (
              <th key={col.name} onClick={() => handleSort(col.name)}>
                {col.name}
                <span className="th-type">{col.type}</span>
                {sortCol === col.name && (
                  <span style={{ color: "var(--brand-cyan)", marginLeft: 3, fontSize: 10 }}>
                    {sortDir === "asc" ? "↑" : "↓"}
                  </span>
                )}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.map((row, i) => (
            <tr
              key={i}
              className={selectedRow === i ? "row-selected" : ""}
              onClick={() => setSelectedRow(i === selectedRow ? null : i)}
            >
              <td className="row-num">{i + 1}</td>
              {columns.map((col) => {
                const val = row[col.name];
                return (
                  <td key={col.name} className={cellClass(val, col.type)}>
                    {formatValue(val)}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
