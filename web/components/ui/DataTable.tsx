"use client";

import { useEffect, useMemo, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { EmptyState } from "./EmptyState";
import { Skeleton } from "./Skeleton";

export type Column<T> = {
  key: string;
  header: ReactNode;
  render: (row: T) => ReactNode;
  className?: string;
  mobileLabel?: string;
};

export type DataTableProps<T> = {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T, index: number) => string;
  loading?: boolean;
  empty?: ReactNode;
  expandable?: (row: T) => ReactNode | null;
  rowClassName?: (row: T) => string;
  pagination?: boolean;
  pageSize?: number;
  pageSizeOptions?: number[];
  pageSizeStorageKey?: string;
  paginationLabels?: {
    previous: string;
    next: string;
    rowsPerPage?: string;
  };
};

export function DataTable<T>({ columns, rows, rowKey, loading, empty, expandable, rowClassName, pagination = false, pageSize = 20, pageSizeOptions, pageSizeStorageKey, paginationLabels }: DataTableProps<T>) {
  const [openRows, setOpenRows] = useState<Record<string, boolean>>({});
  const [page, setPage] = useState(0);
  const normalizedOptions = useMemo(
    () => Array.from(new Set((pageSizeOptions ?? []).map((value) => Math.floor(Number(value))).filter((value) => value > 0))).sort((a, b) => a - b),
    [pageSizeOptions]
  );
  const [selectedPageSize, setSelectedPageSize] = useState(() => normalizePageSize(pageSize, normalizedOptions));
  const normalizedPageSize = normalizePageSize(selectedPageSize, normalizedOptions);
  const totalPages = pagination ? Math.max(1, Math.ceil(rows.length / normalizedPageSize)) : 1;
  const activePage = Math.min(page, totalPages - 1);
  const visibleRows = useMemo(
    () => pagination ? rows.slice(activePage * normalizedPageSize, activePage * normalizedPageSize + normalizedPageSize) : rows,
    [activePage, normalizedPageSize, pagination, rows]
  );

  useEffect(() => {
    if (page > totalPages - 1) setPage(Math.max(0, totalPages - 1));
  }, [page, totalPages]);

  useEffect(() => {
    if (!pageSizeStorageKey || typeof window === "undefined") {
      setSelectedPageSize(normalizePageSize(pageSize, normalizedOptions));
      return;
    }
    const stored = Number(window.localStorage.getItem(pageSizeStorageKey));
    const next = normalizePageSize(Number.isFinite(stored) && stored > 0 ? stored : pageSize, normalizedOptions);
    setSelectedPageSize(next);
  }, [normalizedOptions, pageSize, pageSizeStorageKey]);

  useEffect(() => {
    setOpenRows({});
  }, [activePage, normalizedPageSize, rows]);

  function updatePageSize(nextRaw: number) {
    const next = normalizePageSize(nextRaw, normalizedOptions);
    setSelectedPageSize(next);
    setPage(0);
    if (pageSizeStorageKey && typeof window !== "undefined") {
      window.localStorage.setItem(pageSizeStorageKey, String(next));
    }
  }

  if (loading) {
    return (
      <div className="grid gap-3">
        {Array.from({ length: 4 }).map((_, i) => (
          <Skeleton key={i} className="h-14 w-full rounded-2xl" />
        ))}
      </div>
    );
  }

  if (!rows.length) {
    return empty ?? <EmptyState shape="circle" title="这里还空着" hint="数据出现后会展示在这里。" />;
  }

  return (
    <div className="overflow-hidden rounded-3xl border-2 border-slate-800 bg-white">
      <table className="hidden w-full md:table">
        <thead className="bg-amber-100">
          <tr>
            {expandable && <th className="w-10"></th>}
            {columns.map((col) => (
              <th key={col.key} className={`px-4 py-3 text-left text-xs font-bold uppercase tracking-wider text-slate-700 ${col.className ?? ""}`}>
                {col.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {visibleRows.map((row, index) => {
            const sourceIndex = pagination ? activePage * normalizedPageSize + index : index;
            const k = rowKey(row, sourceIndex);
            const isOpen = openRows[k];
            const expand = expandable?.(row);
            return (
              <Row key={k} k={k} row={row} index={index} columns={columns} expand={expand} isOpen={!!isOpen} setOpen={(v: boolean) => setOpenRows((p) => ({ ...p, [k]: v }))} rowClassName={rowClassName} expandable={!!expandable} />
            );
          })}
        </tbody>
      </table>

      <div className="grid gap-3 p-3 md:hidden">
        {visibleRows.map((row, index) => {
          const sourceIndex = pagination ? activePage * normalizedPageSize + index : index;
          const k = rowKey(row, sourceIndex);
          const expand = expandable?.(row);
          const isOpen = openRows[k];
          const extra = rowClassName?.(row) ?? "";
          return (
            <div key={k} className={`rounded-2xl border-2 border-slate-800 bg-white p-3 ${extra}`}>
              {columns.map((col) => (
                <div key={col.key} className="flex items-start justify-between gap-3 py-1 text-sm">
                  <span className="text-xs font-bold uppercase tracking-wider text-slate-500">{col.mobileLabel ?? col.header}</span>
                  <span className="text-right">{col.render(row)}</span>
                </div>
              ))}
              {expand && (
                <button onClick={() => setOpenRows((p) => ({ ...p, [k]: !isOpen }))} className="mt-2 flex items-center gap-1 text-sm font-bold text-violet-600">
                  {isOpen ? <ChevronDown size={16} /> : <ChevronRight size={16} />} 详情
                </button>
              )}
              {isOpen && expand && <div className="mt-3 rounded-xl bg-slate-50 p-3">{expand}</div>}
            </div>
          );
        })}
      </div>
      {pagination && (rows.length > normalizedPageSize || normalizedOptions.length > 0) && (
        <div className="flex flex-wrap items-center justify-between gap-3 border-t-2 border-slate-800 bg-amber-50 px-4 py-3 text-sm font-bold">
          <span className="font-mono text-xs text-slate-600">
            {activePage * normalizedPageSize + 1}-{Math.min(rows.length, (activePage + 1) * normalizedPageSize)} / {rows.length}
          </span>
          <div className="flex items-center gap-2">
            {normalizedOptions.length > 0 && (
              <label className="flex items-center gap-2 text-xs text-slate-600">
                <span>{paginationLabels?.rowsPerPage ?? "Rows"}</span>
                <select
                  value={normalizedPageSize}
                  onChange={(event) => updatePageSize(Number(event.target.value))}
                  className="rounded-full border-2 border-slate-800 bg-white px-2 py-1 font-mono text-xs outline-none"
                >
                  {normalizedOptions.map((option) => (
                    <option key={option} value={option}>{option}</option>
                  ))}
                </select>
              </label>
            )}
            <button
              type="button"
              onClick={() => setPage((cur) => Math.max(0, cur - 1))}
              disabled={activePage === 0}
              className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 disabled:opacity-40"
            >
              {paginationLabels?.previous ?? "Prev"}
            </button>
            <span className="font-mono text-xs">{activePage + 1} / {totalPages}</span>
            <button
              type="button"
              onClick={() => setPage((cur) => Math.min(totalPages - 1, cur + 1))}
              disabled={activePage >= totalPages - 1}
              className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 disabled:opacity-40"
            >
              {paginationLabels?.next ?? "Next"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function normalizePageSize(value: number, options: number[]): number {
  const fallback = 20;
  const parsed = Math.min(500, Math.max(1, Math.floor(Number(value) || fallback)));
  return options.length > 0 && !options.includes(parsed) ? fallback : parsed;
}

function Row<T>({ k, row, index, columns, expand, isOpen, setOpen, rowClassName, expandable }: { k: string; row: T; index: number; columns: Column<T>[]; expand: ReactNode | null | undefined; isOpen: boolean; setOpen: (v: boolean) => void; rowClassName?: (row: T) => string; expandable: boolean; }) {
  const extra = rowClassName?.(row) ?? "";
  return (
    <>
      <tr className={`border-t-2 border-slate-200 ${index % 2 === 1 ? "bg-amber-50/40" : ""} ${extra}`}>
        {expandable && (
          <td className="w-10 px-2 align-top">
            {expand ? (
              <button onClick={() => setOpen(!isOpen)} className="rounded-full border-2 border-slate-800 bg-white p-1">
                {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              </button>
            ) : null}
          </td>
        )}
        {columns.map((col) => (
          <td key={col.key} className={`px-4 py-3 text-sm align-top ${col.className ?? ""}`}>
            {col.render(row)}
          </td>
        ))}
      </tr>
      {isOpen && expand && (
        <tr className="border-t-2 border-slate-200 bg-slate-50">
          <td colSpan={columns.length + (expandable ? 1 : 0)} className="px-6 py-4">{expand}</td>
        </tr>
      )}
    </>
  );
}
