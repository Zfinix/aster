import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** Renders the icon set as a browsable page: search, both styles, both sizes,
 *  light and dark. Run it after touching icons.tsx and look at the result --
 *  a glyph that reads wrong at 16px next to text is invisible in source. */
const here = dirname(fileURLToPath(import.meta.url));
const SRC = join(here, "../webview/components/icons.tsx");
const OUT = join(here, "../out/icons.html");
mkdirSync(dirname(OUT), { recursive: true });
const src = readFileSync(SRC, "utf8");

/** Concepts come from the components that own them, so the page cannot drift
 *  from what the product actually renders. */
function concepts() {
  const rows = [];
  const grab = (file, where) => {
    let text;
    try { text = readFileSync(join(here, "../webview/components/" + file), "utf8"); } catch { return; }
    const map = /(?:const ICONS[^=]*=\s*\{)([\s\S]*?)\n\};/.exec(text);
    if (map) for (const m of map[1].matchAll(/(\w+):\s*<(\w+)\s*\/>/g))
      rows.push({ concept: m[1], icon: m[2], where });
    for (const m of text.matchAll(/(?:id|mode|name):\s*"([\w-]+)"[\s\S]{0,240}?icon:\s*<(\w+)\s*\/>/g))
      rows.push({ concept: m[1], icon: m[2], where });
  };
  grab("ToolCallRow.tsx", "Tool calls");
  grab("ApprovalPicker.tsx", "Approvals");
  const seen = new Set();
  return rows.filter(r => !seen.has(r.concept + r.icon) && seen.add(r.concept + r.icon));
}
const CONCEPTS = concepts();

const found = [];
const re = /export function (\w+)\([^)]*\)\s*\{[\s\S]*?return \(\s*([\s\S]*?)\s*\);\s*\n\}/g;
let m;
while ((m = re.exec(src))) found.push({ name: m[1], jsx: m[2] });

const kebab = (s) =>
  s.replace(/\b(strokeDasharray|strokeDashoffset|fillOpacity|fillRule|clipRule|strokeWidth|strokeLinecap|strokeLinejoin)=/g,
    (x) => x.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase()));
const body = (jsx) =>
  kebab(jsx.replace(/^<svg[^>]*>/, "").replace(/<\/svg>\s*$/, ""))
    .replace(/\s(style|className)=("[^"]*"|\{[^}]*\})/g, "").replace(/\s+/g, " ").trim();

const icons = new Map();
for (const f of found) {
  const filled = /FilledIcon$/.test(f.name);
  const key = f.name.replace(/(Filled)?Icon$/, "");
  const e = icons.get(key) ?? { key, outline: null, filled: null };
  e[filled ? "filled" : "outline"] = body(f.jsx);
  icons.set(key, e);
}
const list = [...icons.values()].filter((i) => i.outline || i.filled);
const label = (k) => k.replace(/([a-z\d])([A-Z])/g, "$1 $2");
const data = list.map((i) => ({ n: i.key, l: label(i.key), o: i.outline, f: i.filled }));

writeFileSync(OUT, `<!doctype html><meta charset="utf-8"><title>Aster Icons</title>
<style>
 *{box-sizing:border-box}
 [hidden]{display:none!important}
 .grid,.list,.table{outline:none}
 :root{
   --bg:#efeeea;--panel:#fbfaf8;--line:#e3e1db;--fg:#1b1a18;--dim:#8b8880;--accent:#e0603a;
   --chip:#e9e7e1;
 }
 body.dark{--bg:#111110;--panel:#191918;--line:#2a2a28;--fg:#ededeb;--dim:#7e7c76;--chip:#232321}
 body{margin:0;background:var(--bg);color:var(--fg);
   font:14px/1.35 -apple-system,BlinkMacSystemFont,"Segoe UI",Inter,system-ui,sans-serif;
   -webkit-font-smoothing:antialiased}
 .app{display:grid;grid-template-columns:210px 1fr;height:100vh;background:var(--panel)}
 aside{border-right:1px solid var(--line);padding:22px 18px;background:var(--bg)}
 aside h3{font-size:12px;font-weight:400;color:var(--dim);margin:0 0 10px;letter-spacing:.02em}
 aside a{display:block;padding:5px 0;color:var(--fg);text-decoration:none;font-size:14px}
 aside a.on{color:var(--accent)}
 aside .gap{height:26px}
 main{display:flex;flex-direction:column;min-width:0;min-height:0;overflow:hidden}
 .bar{display:flex;align-items:center;gap:0;border-bottom:1px solid var(--line);padding:0 16px;height:56px;flex:none}
 .bar .sep{width:1px;height:26px;background:var(--line);margin:0 16px}
 .search{display:flex;align-items:center;gap:9px;flex:1;min-width:120px}
 .search input{border:0;background:none;color:var(--fg);font:inherit;outline:none;width:100%}
 .search input::placeholder{color:var(--dim)}
 .seg{display:flex;gap:14px;font-size:14px}
 .seg button{border:0;background:none;padding:0;font:inherit;color:var(--dim);cursor:pointer}
 .seg button.on{color:var(--accent)}
 input[type=range]{width:150px;accent-color:var(--accent)}
 .num{border:1px solid var(--line);border-radius:7px;padding:5px 9px;min-width:46px;text-align:center;
   background:var(--bg);font-variant-numeric:tabular-nums}
 .count{color:var(--dim);font-variant-numeric:tabular-nums;white-space:nowrap}
 .ico-btn{border:0;background:none;cursor:pointer;color:var(--dim);padding:6px;display:flex}
 .grid{flex:1;min-height:0;overflow:auto;padding:26px 22px;display:grid;gap:6px;
   grid-template-columns:repeat(auto-fill,minmax(var(--cell),1fr));align-content:start}
 .cell{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:7px;
   aspect-ratio:1;border-radius:9px;cursor:pointer;color:var(--fg);padding:6px;text-align:center}
 .cell:hover{background:var(--chip)}
 .cell.miss{opacity:.22}
 .cell b{font-weight:400;font-size:10px;color:var(--dim);max-width:100%;overflow:hidden;
   text-overflow:ellipsis;white-space:nowrap}
 .foot{border-top:1px solid var(--line);height:46px;display:flex;align-items:center;justify-content:center;
   gap:22px;font-size:13px;color:var(--fg);flex:none}
 .foot label{display:flex;align-items:center;gap:7px;cursor:pointer}
 .empty{color:var(--dim);padding:40px;grid-column:1/-1;text-align:center}
 .list{flex:1;min-height:0;overflow:auto;padding:28px 30px;display:grid;gap:2px 34px;
   grid-template-columns:repeat(auto-fill,minmax(190px,1fr));align-content:start}
 .list .r{display:flex;align-items:center;gap:12px;height:30px;font-size:16px}
 .list .r span{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
 .table{flex:1;min-height:0;overflow:auto;padding:26px 30px}
 .table table{width:100%;border-collapse:collapse;font-size:14px}
 .table th{text-align:left;font-weight:400;color:var(--dim);padding:0 0 12px;font-size:13px}
 .table td{padding:11px 0;border-top:1px solid var(--line);vertical-align:middle}
 .table td:first-child{width:40%}
 .table .c{display:flex;align-items:center;gap:11px}
 code{background:var(--chip);border-radius:6px;padding:3px 8px;
   font:12px ui-monospace,SFMono-Regular,Menlo,monospace}
 .where{color:var(--dim)}
 .scrim{position:fixed;inset:0;background:rgba(0,0,0,.28)}
 .pop{position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);width:330px;
   background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:18px;
   box-shadow:0 22px 60px rgba(0,0,0,.24)}
 .pop-head{display:flex;align-items:center;justify-content:space-between;margin-bottom:6px}
 .pop-head b{font-weight:400;font-size:15px}
 .pop-art{display:flex;align-items:center;justify-content:center;height:150px}
 .pop-acts{display:grid;grid-template-columns:1fr 1fr;gap:8px;margin-top:6px}
 .pop-acts button{border:1px solid var(--line);background:var(--bg);color:var(--fg);
   border-radius:9px;padding:9px;font:inherit;font-size:13px;cursor:pointer}
 .pop-acts button:hover{background:var(--chip)}
 .pop-acts button:last-child{grid-column:1/-1}
 svg{overflow:visible;flex:none}
</style>
<div class="app">
 <aside>
  <h3>Views</h3>
  <a class="on" href="#icons" data-page="icons">Icons</a>
  <a href="#list" data-page="list">List</a>
  <a href="#concepts" data-page="concepts">Concepts</a>
  <div class="gap"></div><h3>Filter</h3>
  <a href="#" data-q="circle">State</a><a href="#" data-q="git">Git</a>
  <a href="#" data-q="arrow">Arrows</a><a href="#" data-q="file">Files</a>
  <div class="gap"></div><h3>System</h3>
  <a href="#" data-q="" style="color:var(--dim);cursor:default">16 grid · 1.25 stroke</a>
  <a href="#" data-q="" style="color:var(--dim);cursor:default">3 unit minimum cuts</a>
 </aside>
 <main>
  <div class="bar">
   <div class="search">
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.25"
      stroke-linecap="round" stroke-linejoin="round" style="color:var(--dim)">
      <circle cx="6.8" cy="6.8" r="4.5"/><path d="M10 10l4 4"/></svg>
    <input id="q" placeholder="Search icons by name…" autocomplete="off">
   </div>
   <div class="sep"></div>
   <div class="seg" id="sizes"><button data-s="16" class="on">16px</button><button data-s="24">24px</button></div>
   <div class="sep"></div>
   <div class="seg" id="styles"><button data-v="o" class="on">Outline</button><button data-v="f">Filled</button></div>
   <div class="sep"></div>
   <input type="range" id="zoom" min="16" max="72" value="36">
   <div class="num"><span id="zv">36</span> <span style="color:var(--dim)">px</span></div>
   <div class="sep"></div>
   <div class="count" id="count"></div>
   <button class="ico-btn" id="theme" title="Theme">
    <svg viewBox="0 0 16 16" width="17" height="17" fill="none" stroke="currentColor" stroke-width="1.25"
      stroke-linecap="round" stroke-linejoin="round"><path d="M13.8 9.7A6.5 6.5 0 016.3 2.2a6.5 6.5 0 107.5 7.5z"/></svg>
   </button>
  </div>
  <div class="grid" id="grid"></div>
  <div class="table" id="table" hidden></div>
  <div class="list" id="list" hidden></div>
  <div class="scrim" id="scrim" hidden></div>
  <div class="pop" id="pop" hidden>
   <div class="pop-head"><b id="pop-name"></b><button class="ico-btn" id="pop-x">
     <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.25"
       stroke-linecap="round"><path d="M3.6 3.6l8.8 8.8M12.4 3.6l-8.8 8.8"/></svg></button></div>
   <div class="pop-art" id="pop-art"></div>
   <div class="pop-acts">
    <button data-act="svg">Copy SVG</button><button data-act="jsx">Copy JSX</button>
    <button data-act="dl">Download SVG</button>
   </div>
  </div>
  <div class="foot">
   <label><input type="checkbox" id="names" checked> Names</label>
   <label><input type="checkbox" id="only"> Has filled</label>
   <label><input type="checkbox" id="box"> Show grid box</label>
  </div>
 </main>
</div>
<script>
const ICONS = ${JSON.stringify(data)};
const CONCEPTS = ${JSON.stringify(CONCEPTS)};
let style = "o", stroke = 1.25, zoom = 36, names = true, only = false, box = false, q = "", page = "icons";
const grid = document.getElementById("grid");

function svg(i){
  const filled = style === "f" && i.f;
  const b = filled ? i.f : (i.o || i.f);
  const at = filled
    ? 'fill="currentColor" stroke="none"'
    : \`fill="none" stroke="currentColor" stroke-width="\${stroke}" stroke-linecap="round" stroke-linejoin="round"\`;
  const g = box ? '<rect x="1.5" y="1.5" width="13" height="13" fill="none" stroke="var(--accent)" stroke-width=".25" opacity=".5"/><circle cx="8" cy="8" r="7" fill="none" stroke="var(--accent)" stroke-width=".25" opacity=".5"/>' : "";
  return \`<svg viewBox="0 0 16 16" width="\${zoom}" height="\${zoom}" \${at}>\${g}\${b}</svg>\`;
}

function markup(i, size, filled){
  const b = filled && i.f ? i.f : (i.o || i.f);
  const at = filled && i.f
    ? 'fill="currentColor" stroke="none"'
    : \`fill="none" stroke="currentColor" stroke-width="\${stroke}" stroke-linecap="round" stroke-linejoin="round"\`;
  return \`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="\${size}" height="\${size}" \${at}>\${b}</svg>\`;
}

let current = null;
function open(i){
  current = i;
  document.getElementById("pop-name").textContent = i.n + (style === "f" && i.f ? "FilledIcon" : "Icon");
  document.getElementById("pop-art").innerHTML = markup(i, 104, style === "f");
  for (const el of [document.getElementById("pop"), document.getElementById("scrim")]) el.hidden = false;
}
function close(){ for (const el of [document.getElementById("pop"), document.getElementById("scrim")]) el.hidden = true; }
document.getElementById("pop-x").onclick = close;
document.getElementById("scrim").onclick = close;
addEventListener("keydown", e => { if (e.key === "Escape") close(); });
for (const b of document.querySelectorAll(".pop-acts button")) b.onclick = async () => {
  if (!current) return;
  const name = current.n + (style === "f" && current.f ? "FilledIcon" : "Icon");
  const svgText = markup(current, 16, style === "f");
  if (b.dataset.act === "dl") {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([svgText], { type: "image/svg+xml" }));
    a.download = name + ".svg"; a.click(); URL.revokeObjectURL(a.href);
  } else {
    const text = b.dataset.act === "jsx" ? \`<\${name} />\` : svgText;
    await navigator.clipboard.writeText(text).catch(() => {});
    const was = b.textContent; b.textContent = "Copied"; setTimeout(() => (b.textContent = was), 900);
  }
};

function renderList(){
  const t = q.trim().toLowerCase();
  const rows = ICONS.filter(i => (!only || i.f) && (!t || i.n.toLowerCase().includes(t)));
  document.getElementById("list").innerHTML = rows.length
    ? rows.map(i => \`<div class="r">\${markup(i, 16, style === "f")}<span>\${i.l}</span></div>\`).join("")
    : '<div class="empty">No icon matches that.</div>';
  document.getElementById("count").textContent = rows.length + " / " + ICONS.length;
}

function renderConcepts(){
  const byKey = Object.fromEntries(ICONS.map(i => [i.n, i]));
  const t = q.trim().toLowerCase();
  const rows = CONCEPTS.filter(c => !t || c.concept.toLowerCase().includes(t) || c.icon.toLowerCase().includes(t));
  document.getElementById("table").innerHTML =
    \`<table><thead><tr><th>Concept</th><th>Icon</th><th>Used by</th></tr></thead><tbody>\` +
    rows.map(c => {
      const i = byKey[c.icon.replace(/Icon$/, "")];
      return \`<tr><td><div class="c">\${i ? markup(i, 17, false) : ""}<span>\${c.concept}</span></div></td>
        <td><code>\${c.icon}</code></td><td class="where">\${c.where}</td></tr>\`;
    }).join("") + \`</tbody></table>\`;
  document.getElementById("count").textContent = rows.length + " / " + CONCEPTS.length;
}

function render(){
  document.getElementById("grid").hidden = page !== "icons";
  document.getElementById("list").hidden = page !== "list";
  document.getElementById("table").hidden = page !== "concepts";
  document.getElementById("q").placeholder =
    page === "concepts" ? "Search concepts…" : "Search icons by name…";
  if (page === "concepts") return renderConcepts();
  if (page === "list") return renderList();
  const t = q.trim().toLowerCase();
  const rows = ICONS.filter(i => (!only || i.f) && (!t || i.n.toLowerCase().includes(t)));
  grid.style.setProperty("--cell", (zoom + (names ? 46 : 26)) + "px");
  grid.innerHTML = rows.length
    ? rows.map((i, x) => \`<div class="cell \${style==="f"&&!i.f?"miss":""}" data-x="\${x}" title="\${i.n}Icon">\${svg(i)}\${names?\`<b>\${i.l}</b>\`:""}</div>\`).join("")
    : '<div class="empty">No icon matches that.</div>';
  document.getElementById("count").textContent = rows.length + " / " + ICONS.length;
  for (const c of grid.querySelectorAll(".cell")) c.onclick = () => open(rows[+c.dataset.x]);
}
document.getElementById("q").oninput = e => { q = e.target.value; render(); };
document.getElementById("zoom").oninput = e => { zoom = +e.target.value; document.getElementById("zv").textContent = zoom; render(); };
document.getElementById("names").onchange = e => { names = e.target.checked; render(); };
document.getElementById("only").onchange = e => { only = e.target.checked; render(); };
document.getElementById("box").onchange = e => { box = e.target.checked; render(); };
document.getElementById("theme").onclick = () => document.body.classList.toggle("dark");
for (const b of document.querySelectorAll("#sizes button")) b.onclick = () => {
  document.querySelectorAll("#sizes button").forEach(x => x.classList.remove("on"));
  b.classList.add("on"); stroke = b.dataset.s === "24" ? 1.5 : 1.25; render();
};
for (const b of document.querySelectorAll("#styles button")) b.onclick = () => {
  document.querySelectorAll("#styles button").forEach(x => x.classList.remove("on"));
  b.classList.add("on"); style = b.dataset.v; render();
};
for (const a of document.querySelectorAll("aside a")) a.onclick = e => {
  e.preventDefault();
  document.querySelectorAll("aside a").forEach(x => x.classList.remove("on"));
  a.classList.add("on");
  if (a.dataset.page) { page = a.dataset.page; q = ""; location.hash = page; }
  else q = a.dataset.q ?? "";
  document.getElementById("q").value = q; render();
};
/** Hash routing so a view can be linked, and screenshotted, directly. */
function route(){
  const h = location.hash.replace("#", "");
  if (["icons", "list", "concepts"].includes(h)) page = h;
  for (const a of document.querySelectorAll("aside a[data-page]"))
    a.classList.toggle("on", a.dataset.page === page);
  render();
}
addEventListener("hashchange", route);
route();
</script>`);
console.log(`${list.length} icons (${list.filter(i=>i.filled).length} filled) -> ${OUT}`);
