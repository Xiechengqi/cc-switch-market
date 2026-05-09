export async function apiGet<T>(path: string, _admin = false): Promise<T> {
  const res = await fetch(path, {
    credentials: "include"
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export type Page<T> = {
  items: T[];
  nextCursor?: string | null;
  hasMore?: boolean;
};

export async function apiGetItems<T>(path: string, admin = false): Promise<T[]> {
  const value = await apiGet<T[] | Page<T>>(path, admin);
  if (Array.isArray(value)) return value;
  return value.items ?? [];
}

export async function apiGetAllItems<T>(path: string, admin = false): Promise<T[]> {
  const items: T[] = [];
  let cursor: string | null | undefined;
  let guard = 0;
  do {
    const separator = path.includes("?") ? "&" : "?";
    const url = `${path}${separator}limit=200${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`;
    const value = await apiGet<T[] | Page<T>>(url, admin);
    if (Array.isArray(value)) return value;
    items.push(...(value.items ?? []));
    cursor = value.nextCursor;
    guard += 1;
  } while (cursor && guard < 50);
  return items;
}

export async function apiPost<T>(path: string, body: unknown, _admin = false): Promise<T> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json", ...csrfHeader() },
    credentials: "include",
    body: JSON.stringify(body)
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function apiPutBytes<T>(path: string, body: File): Promise<T> {
  const res = await fetch(path, {
    method: "PUT",
    headers: csrfHeader(),
    credentials: "include",
    body
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function apiPutJson<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method: "PUT",
    headers: { "content-type": "application/json", ...csrfHeader() },
    credentials: "include",
    body: JSON.stringify(body)
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function apiPatchJson<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(path, {
    method: "PATCH",
    headers: { "content-type": "application/json", ...csrfHeader() },
    credentials: "include",
    body: JSON.stringify(body)
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function apiDelete<T>(path: string, body?: unknown): Promise<T> {
  const hasBody = body !== undefined;
  const res = await fetch(path, {
    method: "DELETE",
    headers: { ...(hasBody ? { "content-type": "application/json" } : {}), ...csrfHeader() },
    credentials: "include",
    body: hasBody ? JSON.stringify(body) : undefined
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

function csrfHeader(): Record<string, string> {
  if (typeof document === "undefined") return {};
  const token = document.cookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("cc_switch_market_csrf="))
    ?.split("=")
    .slice(1)
    .join("=");
  return token ? { "x-csrf-token": decodeURIComponent(token) } : {};
}
