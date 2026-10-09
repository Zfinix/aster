import "maplibre-gl/dist/maplibre-gl.css";

import maplibregl from "maplibre-gl";
import { useEffect, useRef, useState } from "react";
import { useColorMode } from "../lib/colorMode";
import { placed, type Place } from "../lib/places";

/** OpenFreeMap's hosted styles: free vector tiles from OpenStreetMap, no key. */
const STYLES = {
  light: "https://tiles.openfreemap.org/styles/positron",
  dark: "https://tiles.openfreemap.org/styles/dark",
};
const ROUTE = "aster-route";
const ONE_STOP_ZOOM = 13;

function pin(place: Place, index: number): HTMLElement {
  const el = document.createElement("div");
  el.className = "openui-map-pin";
  el.textContent = place.emoji?.trim() || String(index + 1);
  el.title = place.name;
  return el;
}

/** A street map with a pin per place, framed to fit them. Places stream in one
 *  at a time; the view settles on all of them once the reply is done. */
export function OpenUIMap({ places, route, streaming }: { places: Partial<Place>[]; route: boolean; streaming: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const [map, setMap] = useState<maplibregl.Map | null>(null);
  const mode = useColorMode();
  const ready = placed(places);
  const key = JSON.stringify(ready.map((p) => [p.lat, p.lng, p.emoji]));

  useEffect(() => {
    if (!box.current) return;
    const created = new maplibregl.Map({
      container: box.current,
      style: STYLES[mode],
      center: [0, 20],
      zoom: 1,
      attributionControl: { compact: true },
      cooperativeGestures: true,
    });
    created.addControl(new maplibregl.NavigationControl({ showCompass: false }), "top-right");
    created.once("load", () => setMap(created));
    return () => {
      setMap(null);
      created.remove();
    };
  }, [mode]);

  useEffect(() => {
    if (!map || !ready.length) return;
    const markers = ready.map((place, i) =>
      new maplibregl.Marker({ element: pin(place, i) })
        .setLngLat([place.lng, place.lat])
        .setPopup(new maplibregl.Popup({ offset: 18, closeButton: false }).setText(place.name))
        .addTo(map)
    );
    const line = route && ready.length > 1;
    if (line) {
      map.addSource(ROUTE, {
        type: "geojson",
        data: {
          type: "Feature",
          properties: {},
          geometry: { type: "LineString", coordinates: ready.map((p) => [p.lng, p.lat]) },
        },
      });
      map.addLayer({
        id: ROUTE,
        type: "line",
        source: ROUTE,
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#3b82f6", "line-width": 3, "line-dasharray": [1.5, 1.5] },
      });
    }
    if (ready.length === 1) {
      map.jumpTo({ center: [ready[0].lng, ready[0].lat], zoom: ONE_STOP_ZOOM });
    } else {
      const bounds = new maplibregl.LngLatBounds();
      for (const place of ready) bounds.extend([place.lng, place.lat]);
      map.fitBounds(bounds, { padding: 48, maxZoom: 15, duration: streaming ? 0 : 600 });
    }
    return () => {
      for (const marker of markers) marker.remove();
      if (line) {
        map.removeLayer(ROUTE);
        map.removeSource(ROUTE);
      }
    };
    // `key` stands in for `ready`, which is a new array every render.
  }, [map, key, route, streaming]);

  return <div className="openui-map" ref={box} />;
}
