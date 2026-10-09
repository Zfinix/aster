/** Stands in for rehype-katex, which OpenUI's markdown loads on demand: a
 *  second chunk the single-file panel cannot load. Math stays as written. */
export default function rehypeKatex() {
  return () => {};
}
