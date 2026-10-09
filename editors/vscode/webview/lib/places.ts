/** A place the model put on a map or in an itinerary. Every field can be
 *  missing while the reply is still streaming. */
export interface Place {
  name: string;
  lat: number;
  lng: number;
  emoji?: string;
  time?: string;
  note?: string;
  image?: string;
}

/** The places whose coordinates are real numbers inside the world's bounds,
 *  which is what a half-streamed or mistaken stop is filtered on. */
export function placed(places: Partial<Place>[]): Place[] {
  return places.filter(
    (p): p is Place =>
      typeof p.name === "string" &&
      typeof p.lat === "number" &&
      typeof p.lng === "number" &&
      Math.abs(p.lat) <= 90 &&
      Math.abs(p.lng) <= 180
  );
}

/** The props of each child node the renderer hands a component, whether it
 *  arrives as an element node or as plain props. */
export function childProps<T>(items: unknown): Partial<T>[] {
  if (!Array.isArray(items)) return [];
  return items.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const props = "props" in item ? (item as { props: unknown }).props : item;
    return props && typeof props === "object" ? [props as Partial<T>] : [];
  });
}

/** An http(s) URL, or nothing: model output never reaches an `src` unchecked. */
export function safeUrl(value: unknown): string | undefined {
  if (typeof value !== "string" || !value.trim()) return undefined;
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:" ? url.href : undefined;
  } catch {
    return undefined;
  }
}
