import { useState } from "react";
import { safeUrl, type Place } from "../lib/places";

/** One row of an itinerary: a photo when there is one that loads, the place's
 *  name and time, and a short note. */
export function OpenUIStop({ place, index }: { place: Partial<Place>; index: number }) {
  const [broken, setBroken] = useState(false);
  const src = safeUrl(place.image);
  return (
    <div className="openui-stop">
      {src && !broken ? (
        <img
          className="openui-stop-photo"
          src={src}
          alt={place.name ?? ""}
          referrerPolicy="no-referrer"
          onError={() => setBroken(true)}
        />
      ) : (
        <span className="openui-stop-mark" aria-hidden="true">
          {place.emoji?.trim() || index + 1}
        </span>
      )}
      <div className="openui-stop-text">
        <div className="openui-stop-head">
          <span className="openui-stop-name">{place.name}</span>
          {place.time && <span className="openui-stop-time">{place.time}</span>}
        </div>
        {place.note && <div className="openui-stop-note">{place.note}</div>}
      </div>
    </div>
  );
}
