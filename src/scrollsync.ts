// 原文 (textarea) と訳文 (div) のスクロール位置を同期する。
//
// 翻訳は全文一括なので、両者の行 (行数が合わなければ段落) を対応づけ、
// 各ブロックの表示位置をアンカーにして区間ごとに線形補間する。
// 対応が取れないときはスクロール率で合わせる。

type Pane = HTMLTextAreaElement | HTMLElement;

function textOf(el: Pane): string {
  return el instanceof HTMLTextAreaElement ? el.value : el.textContent ?? "";
}

/** 各行の開始位置 (空行も含む) */
function lineStarts(text: string): number[] {
  const starts = [0];
  for (let i = text.indexOf("\n"); i !== -1; i = text.indexOf("\n", i + 1)) starts.push(i + 1);
  return starts;
}

/** 各段落 (空行で区切られた非空行のまとまり) の開始位置 */
function paragraphStarts(text: string): number[] {
  const starts: number[] = [];
  let prevBlank = true;
  for (const s of lineStarts(text)) {
    const end = text.indexOf("\n", s);
    const blank = !text.slice(s, end === -1 ? undefined : end).trim();
    if (!blank && prevBlank) starts.push(s);
    prevBlank = blank;
  }
  return starts;
}

const COPY_PROPS = [
  "fontFamily",
  "fontSize",
  "fontWeight",
  "fontStyle",
  "lineHeight",
  "letterSpacing",
  "wordSpacing",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "whiteSpace",
  "overflowWrap",
  "wordBreak",
  "tabSize",
  "textIndent",
] as const;

let mirror: HTMLDivElement | null = null;

/** el と同じ折り返しで text を配置したとき、各 offset の行頭 y 座標 (padding 含む) */
function measure(el: Pane, text: string, offsets: number[]): number[] {
  if (!mirror) {
    mirror = document.createElement("div");
    Object.assign(mirror.style, {
      position: "absolute",
      visibility: "hidden",
      top: "0",
      left: "-99999px",
      height: "auto",
      border: "none",
      boxSizing: "border-box",
      overflow: "hidden",
    });
    document.body.appendChild(mirror);
  }
  const cs = getComputedStyle(el);
  for (const p of COPY_PROPS) mirror.style[p] = cs[p];
  mirror.style.width = `${el.clientWidth}px`;
  mirror.replaceChildren();
  const markers: HTMLSpanElement[] = [];
  let pos = 0;
  for (const off of offsets) {
    if (off > pos) mirror.appendChild(document.createTextNode(text.slice(pos, off)));
    const m = document.createElement("span");
    markers.push(m);
    mirror.appendChild(m);
    pos = off;
  }
  mirror.appendChild(document.createTextNode(text.slice(pos) || "​"));
  const tops = markers.map((m) => m.offsetTop);
  mirror.replaceChildren();
  return tops;
}

interface Anchors {
  key: string;
  src: number[];
  dst: number[];
}

export function setupScrollSync(source: Pane, target: Pane) {
  let driver: Pane = source;
  let cache: Anchors | null = null;
  let frame = 0;

  const anchors = (): Anchors | null => {
    const a = textOf(source);
    const b = textOf(target);
    const key = `${source.clientWidth}|${target.clientWidth}|${a}\u0000${b}`;
    if (cache?.key === key) return cache;
    let sa = lineStarts(a);
    let sb = lineStarts(b);
    if (sa.length !== sb.length) {
      sa = paragraphStarts(a);
      sb = paragraphStarts(b);
    }
    if (sa.length !== sb.length || sa.length === 0) {
      cache = { key, src: [], dst: [] };
    } else {
      cache = { key, src: measure(source, a, sa), dst: measure(target, b, sb) };
    }
    return cache;
  };

  const sync = (from: Pane, to: Pane) => {
    const fromMax = from.scrollHeight - from.clientHeight;
    const toMax = to.scrollHeight - to.clientHeight;
    if (toMax <= 0) return;
    if (fromMax <= 0) {
      to.scrollTop = 0;
      return;
    }
    const y = from.scrollTop;
    const anc = anchors();
    const fromTops = from === source ? anc?.src ?? [] : anc?.dst ?? [];
    const toTops = from === source ? anc?.dst ?? [] : anc?.src ?? [];
    const padFrom = parseFloat(getComputedStyle(from).paddingTop) || 0;
    const padTo = parseFloat(getComputedStyle(to).paddingTop) || 0;

    // 単調増加なアンカー列 (0,0) … (fromMax,toMax) を作って補間する
    const xs = [0];
    const ys = [0];
    for (let i = 0; i < fromTops.length; i++) {
      const fx = fromTops[i] - padFrom;
      const ty = toTops[i] - padTo;
      if (fx > xs[xs.length - 1] && fx < fromMax && ty > ys[ys.length - 1] && ty < toMax) {
        xs.push(fx);
        ys.push(ty);
      }
    }
    xs.push(fromMax);
    ys.push(toMax);

    let i = 1;
    while (i < xs.length - 1 && xs[i] < y) i++;
    const x0 = xs[i - 1];
    const x1 = xs[i];
    const t = x1 > x0 ? Math.min(1, Math.max(0, (y - x0) / (x1 - x0))) : 0;
    const next = ys[i - 1] + (ys[i] - ys[i - 1]) * t;
    if (Math.abs(to.scrollTop - next) >= 1) to.scrollTop = next;
  };

  const schedule = (from: Pane) => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => sync(from, from === source ? target : source));
  };

  for (const el of [source, target]) {
    // ユーザーが操作した側を同期元にする (プログラムによるスクロールで逆流させない)
    for (const ev of ["wheel", "pointerdown", "touchstart", "keydown", "focus"]) {
      el.addEventListener(ev, () => (driver = el), { passive: true });
    }
    el.addEventListener("scroll", () => {
      if (el === driver) schedule(el);
    });
  }

  const ro = new ResizeObserver(() => {
    cache = null;
    schedule(driver);
  });
  ro.observe(source);
  ro.observe(target);

  return {
    /** 訳文の更新後などに、現在の同期元に合わせ直す */
    resync() {
      schedule(driver);
    },
  };
}
