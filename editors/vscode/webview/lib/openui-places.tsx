import { defineComponent, useIsStreaming } from "@openuidev/react-lang";
import { openuiChatLibrary } from "@openuidev/react-ui/genui-lib";
import { z } from "zod/v4";
import { OpenUIMap } from "../components/OpenUIMap";
import { OpenUIStop } from "../components/OpenUIStop";
import { childProps, type Place } from "./places";

/** Most stops one map or itinerary takes, so a runaway list cannot stall the panel. */
const MAX_STOPS = 30;

export const Stop = defineComponent({
  name: "Stop",
  props: z.object({
    name: z.string(),
    lat: z.number().min(-90).max(90),
    lng: z.number().min(-180).max(180),
    emoji: z.string().optional().describe("One emoji for the map pin, such as 🏛️ 🌳 ☕"),
    time: z.string().optional().describe("When or how long, such as 10:00 or 1.5 hrs"),
    note: z.string().optional(),
    image: z.string().optional().describe("A photo URL from web/images, copied exactly; omit when there is none"),
  }),
  description:
    "A real place with accurate coordinates. Define each stop once and reference it from both Map and Itinerary.",
  component: ({ props }) => <OpenUIStop place={props} index={0} />,
});

export const Itinerary = defineComponent({
  name: "Itinerary",
  props: z.object({ title: z.string(), stops: z.array(Stop.ref).max(MAX_STOPS) }),
  description: "An ordered list of stops with photo, time and note. Reuse the stop references the Map uses.",
  component: ({ props }) => (
    <section className="openui-itinerary">
      {props.title && <div className="openui-itinerary-title">{props.title}</div>}
      {childProps<Place>(props.stops).map((place, i) => (
        <OpenUIStop key={`${place.name ?? "stop"}-${i}`} place={place} index={i} />
      ))}
    </section>
  ),
});

export const Map = defineComponent({
  name: "Map",
  props: z.object({
    stops: z.array(Stop.ref).max(MAX_STOPS),
    route: z.boolean().optional().describe("true joins the stops in order with a line; a sketch, not directions"),
  }),
  description:
    "An interactive street map with a pin per stop, framed to fit them. Put it before the stop definitions so pins appear while the reply streams.",
  component: function MapComponent({ props }) {
    const streaming = useIsStreaming();
    return <OpenUIMap places={childProps<Place>(props.stops)} route={props.route ?? false} streaming={streaming} />;
  },
});

const base = openuiChatLibrary.components.Card;

/** OpenUI's Card, taught to hold the place components alongside its own. */
export const Card = defineComponent({
  name: "Card",
  props: base.props.extend({
    children: z.array(z.union([...base.props.shape.children.element.options, Map.ref, Itinerary.ref, Stop.ref])),
  }),
  description: base.description,
  component: base.component,
});
