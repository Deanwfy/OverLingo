#!/usr/bin/env node
// Produces every screenshot and recording the README and the site use, from the browser
// preview, so they all come from the same UI and can be regenerated after a UI change.
//
//   node scripts/record-demo.mjs [--lang en|zh] [--fps 12]
//
// Serves the preview with Vite, drives headless Chrome over the DevTools protocol, and
// lets the mock backend play its demo scenes. Needs Chrome, ffmpeg, and img2webp.
//
//   docs/assets/demo.webm, demo.mov       overlay demo with alpha (VP9 / HEVC for Safari)
//   docs/assets/demo-poster.png           its last frame, transparent
//   docs/assets/social.png                1280x640 card for link previews
//   .github/assets/demo.webp              the demo as an animated WebP for the README
//   docs/assets/video.png, start.png      overlay stills for the site
//   docs/assets + .github/assets/
//     overlay-merged.png, overlay-settings.png, main-translators.png, main-history.png
//
// The demo recording carries a drop shadow because it floats over the page; the stills
// sit in the flow of the page and are captured without one.

import { spawn } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const args = Object.fromEntries(
    process.argv.slice(2).map((a, i, all) => (a.startsWith("--") ? [a.slice(2), all[i + 1]] : [])).filter((p) => p.length),
);
const lang = args.lang ?? "en";
const fps = Number(args.fps ?? 12);
const SLOWDOWN = 4; // the demo runs at 1/4 speed while frames are captured; timing is scaled back on encode
const PORT = 1421; // not 1420, so a running `npm run dev` is left alone
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const root = resolve(fileURLToPath(import.meta.url), "../..");
const docs = (name) => join(root, "docs/assets", name);
const github = (name) => join(root, ".github/assets", name);

// The overlay window and, for the recording, the room its shadow needs to fade out.
const PANEL = { width: 760, height: 250 };
const SHADOW = { css: "0 12px 32px rgba(0, 0, 0, .22)", top: 40, side: 50, bottom: 70 };
const MAIN = { width: 980, height: 650 };

const overlayStage = ({ shadow = false, chrome = "shown", pad }) => `
    /* overlay.css clips html, body, and #overlay; the shadow has to escape the panel box. */
    html, body, #overlay { overflow: visible !important; }
    body { padding: ${pad.top}px ${pad.side}px ${pad.bottom}px !important; }
    .overlay-shell { box-shadow: ${shadow ? `${SHADOW.css}, inset 0 1px 0 rgba(255, 255, 255, .07)` : "none"}; }
    .overlay-reveal { opacity: ${chrome === "hidden" ? 0 : 1} !important; transform: none !important; }
`;
// The main window as macOS frames it: rounded corners and traffic lights in the title bar.
const mainStage = `
    html, body { background: transparent !important; }
    .app-shell { background: var(--background); border-radius: 10px; overflow: hidden; border: 1px solid rgba(0, 0, 0, .12); }
    .traffic-light-space { position: relative; width: 56px; }
    .traffic-light-space::before {
        content: ""; position: absolute; left: 0; top: 50%; width: 12px; height: 12px; border-radius: 50%;
        background: #ff5f57; box-shadow: 20px 0 0 #febc2e, 40px 0 0 #28c840; transform: translateY(-50%);
    }
`;
// The social card: the brand over the wallpaper, with the final frame of the demo laid out
// so the panel itself is 1132px wide with its top edge at 186px.
const socialCard = (poster) => {
    const canvas = { width: PANEL.width + 2 * SHADOW.side, height: PANEL.height + SHADOW.top + SHADOW.bottom };
    const width = (1132 / PANEL.width) * canvas.width;
    const top = 186 - (SHADOW.top / canvas.width) * width;
    return `<!doctype html><meta charset="utf-8">
<style>
  html, body { margin: 0; width: 1280px; height: 640px; overflow: hidden; }
  body { position: relative; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif; color: #1d1d1f;
    background: radial-gradient(55% 60% at 12% 0%, rgba(73, 105, 223, .16), transparent 70%),
      radial-gradient(45% 50% at 90% 20%, rgba(35, 138, 89, .1), transparent 70%),
      linear-gradient(180deg, #eceef5 0%, #e4e7f0 100%); }
  .brand { position: absolute; left: 0; right: 0; top: 52px; display: flex; justify-content: center; align-items: center; gap: 20px; }
  .brand img { width: 88px; height: 88px; filter: drop-shadow(0 8px 20px rgba(36, 55, 143, .25)); }
  .brand h1 { margin: 0; font-size: 68px; font-weight: 700; letter-spacing: -.02em; line-height: 1; }
  .panel { position: absolute; left: 50%; top: ${top.toFixed(1)}px; width: ${width.toFixed(1)}px; transform: translateX(-50%); }
</style>
<div class="brand"><img src="file://${join(root, "docs/icon.svg")}"><h1>OverLingo</h1></div>
<img class="panel" src="file://${poster}">`;
};

const work = mkdtempSync(join(tmpdir(), "overlingo-demo-"));
const children = [];
const stopChildren = () =>
    Promise.all(children.map((c) => new Promise((done) => (c.exitCode === null ? (c.once("exit", done), c.kill()) : done()))));
process.on("exit", () => {
    children.forEach((c) => c.kill());
    rmSync(work, { recursive: true, force: true, maxRetries: 5 });
});
process.on("SIGINT", () => process.exit(130));

const vite = spawn(join(root, "node_modules/.bin/vite"), ["--host", "127.0.0.1", "--port", String(PORT), "--strictPort"], {
    cwd: root,
    stdio: ["ignore", "ignore", "inherit"],
});
children.push(vite);
const origin = `http://127.0.0.1:${PORT}`;
for (let tries = 0; ; tries++) {
    try {
        if ((await fetch(origin + "/overlay.html")).ok) break;
    } catch {}
    if (tries > 100) throw new Error("Vite did not start");
    await new Promise((r) => setTimeout(r, 200));
}

const chrome = spawn(CHROME, [
    "--headless=new",
    "--disable-gpu",
    "--hide-scrollbars",
    "--remote-debugging-port=0",
    `--user-data-dir=${join(work, "profile")}`,
    "about:blank",
]);
children.push(chrome);
const wsUrl = await new Promise((resolveUrl, reject) => {
    let text = "";
    chrome.stderr.on("data", (chunk) => {
        text += chunk;
        const m = text.match(/DevTools listening on (ws:\/\/\S+)/);
        if (m) resolveUrl(m[1]);
    });
    chrome.on("exit", (code) => reject(new Error(`Chrome exited early (${code})\n${text}`)));
});
const port = new URL(wsUrl).port;
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const target = targets.find((t) => t.type === "page");

const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let nextId = 1;
const pending = new Map();
const events = new Map();
ws.addEventListener("message", (e) => {
    const msg = JSON.parse(e.data);
    if (msg.id) {
        const { resolve: ok, reject } = pending.get(msg.id);
        pending.delete(msg.id);
        msg.error ? reject(new Error(msg.error.message)) : ok(msg.result);
    } else {
        events.get(msg.method)?.forEach((fn) => fn(msg.params));
    }
});
const send = (method, params = {}) =>
    new Promise((ok, reject) => {
        const id = nextId++;
        pending.set(id, { resolve: ok, reject });
        ws.send(JSON.stringify({ id, method, params }));
    });
const once = (method) =>
    new Promise((ok) => {
        const fn = (p) => {
            events.get(method).delete(fn);
            ok(p);
        };
        (events.get(method) ?? events.set(method, new Set()).get(method)).add(fn);
    });
const evaluate = async (expression) =>
    (await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true })).result.value;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const screenshot = async (file) => {
    const { data } = await send("Page.captureScreenshot", { format: "png", fromSurface: true });
    writeFileSync(file, Buffer.from(data, "base64"));
};
const waitFor = async (expression, timeout = 20000) => {
    for (let waited = 0; waited < timeout; waited += 100) {
        if (await evaluate(expression)) return;
        await sleep(100);
    }
    throw new Error(`Timed out waiting for ${expression}`);
};

await send("Page.enable");
// Chrome on macOS ignores --lang; the app reads navigator.languages and Intl's default
// locale, which these two set.
const locale = lang === "zh" ? "zh-CN" : "en-US";
const { userAgent } = await send("Browser.getVersion");
await send("Emulation.setUserAgentOverride", { userAgent, acceptLanguage: `${locale},${lang}` });
await send("Emulation.setLocaleOverride", { locale });
// The pages keep their html transparent, so with no default canvas colour the captures carry alpha.
await send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });

let stagedScript = null;
async function open({ url, width, height, stage, scale = 2 }) {
    if (stagedScript) await send("Page.removeScriptToEvaluateOnNewDocument", { identifier: stagedScript });
    stagedScript = null;
    if (stage) {
        ({ identifier: stagedScript } = await send("Page.addScriptToEvaluateOnNewDocument", {
            source: `document.addEventListener("DOMContentLoaded", () => {
                const style = document.createElement("style");
                style.textContent = ${JSON.stringify(stage)};
                document.head.appendChild(style);
            });`,
        }));
    }
    await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: scale, mobile: false });
    const loaded = once("Page.loadEventFired");
    await send("Page.navigate", { url });
    await loaded;
    await sleep(300);
}

// Overlay pages. The saved config is dropped first so one scene's layout cannot leak into
// the next; saved sessions are kept, the history screenshot wants them.
async function openOverlay({ scene = "meeting", speed, layout, shadow = false, chrome = "shown", panel = PANEL }) {
    const pad = shadow ? SHADOW : { top: 0, side: 0, bottom: 0 };
    await evaluate(`localStorage.removeItem("overlingo-config"); true`);
    await open({
        url: `${origin}/overlay.html?autostart&scene=${scene}&speed=${speed}${layout ? `&layout=${layout}` : ""}`,
        width: panel.width + 2 * pad.side,
        height: panel.height + pad.top + pad.bottom,
        stage: overlayStage({ shadow, chrome, pad }),
    });
}
const demoFinished = () => waitFor(`document.documentElement.dataset.demoFinished === "true"`, 60000);
// Stopping makes the mock save the session, the way the app does.
const stopDemo = () => evaluate(`document.querySelector(".stop-translation").click(); true`);
const stillTo = async (...files) => {
    await screenshot(files[0]);
    for (const file of files.slice(1)) copyFileSync(files[0], file);
};

// 1. The demo, recorded frame by frame at quarter speed.
await open({ url: `${origin}/overlay.html`, width: 100, height: 100 });
await evaluate(`localStorage.clear(); true`);
await openOverlay({ speed: 1 / SLOWDOWN, shadow: true });
console.log("labels:", await evaluate(`[...document.querySelectorAll(".route-direction, .translation-control")].map((n) => n.textContent.trim()).join(" | ")`));
const frames = [];
const interval = (1000 / fps) * SLOWDOWN;
const started = Date.now();
while (true) {
    const slot = Date.now();
    const file = join(work, `f${String(frames.length).padStart(4, "0")}.png`);
    await screenshot(file);
    frames.push({ file, t: (slot - started) / SLOWDOWN });
    if ((await evaluate(`document.documentElement.dataset.demoFinished`)) === "true") break;
    const spent = Date.now() - slot;
    if (spent < interval) await sleep(interval - spent);
}
await stopDemo();
copyFileSync(frames.at(-1).file, docs("demo-poster.png"));

// 2. The social card.
const card = join(work, "social.html");
writeFileSync(card, socialCard(docs("demo-poster.png")));
await open({ url: "file://" + card, width: 1280, height: 640, scale: 1 });
await screenshot(docs("social.png"));

// 3. Overlay stills.
await openOverlay({ speed: 4 });
await demoFinished();
await stillTo(docs("start.png"));

await openOverlay({ scene: "video", speed: 4, chrome: "hidden" });
await demoFinished();
await stillTo(docs("video.png"));
await stopDemo();

await openOverlay({ speed: 4, layout: "merged" });
await demoFinished();
await stillTo(docs("overlay-merged.png"), github("overlay-merged.png"));

await openOverlay({ speed: 4, panel: { width: 1080, height: 580 } });
await demoFinished();
await evaluate(`document.querySelector(".overlay-chrome button[aria-expanded]").click(); true`);
await sleep(400);
await stillTo(docs("overlay-settings.png"), github("overlay-settings.png"));

// 4. Main window stills.
const openMain = async () => {
    await open({ url: `${origin}/index.html`, width: MAIN.width, height: MAIN.height, stage: mainStage });
    await waitFor(`!!document.querySelector(".app-shell")`);
};
await openMain();
await evaluate(`[...document.querySelectorAll(".translator-sidebar nav button")].find((b) => b.textContent.includes("Qwen")).click(); true`);
await sleep(300);
await stillTo(docs("main-translators.png"), github("main-translators.png"));

await openMain();
await evaluate(`document.querySelectorAll(".primary-navigation button")[1].click(); true`);
await waitFor(`!!document.querySelector(".history-item")`);
await evaluate(`document.querySelector(".history-item").click(); true`);
await waitFor(`!!document.querySelector(".history-detail h2")`);
await sleep(300);
console.log("history:", await evaluate(`[...document.querySelectorAll(".history-item")].map((n) => n.innerText.replace(/\\n/g, " · ")).join(" | ")`));
await stillTo(docs("main-history.png"), github("main-history.png"));
ws.close();

// 5. Encode the demo.
const list = frames
    .map((f, i) => {
        const next = frames[i + 1]?.t ?? f.t + 1000 / fps;
        return `file '${f.file}'\nduration ${((next - f.t) / 1000).toFixed(3)}`;
    })
    .join("\n");
const framesTxt = join(work, "frames.txt");
writeFileSync(framesTxt, list + `\nfile '${frames.at(-1).file}'\n`);
const ffmpeg = (ffArgs) =>
    new Promise((ok, reject) => {
        const ff = spawn("ffmpeg", ["-y", "-loglevel", "error", ...ffArgs], { stdio: "inherit" });
        ff.on("exit", (code) => (code === 0 ? ok() : reject(new Error(`ffmpeg exited ${code}`))));
    });
const frameInput = ["-f", "concat", "-safe", "0", "-i", framesTxt];
await ffmpeg([...frameInput, "-vf", `fps=${fps}`, "-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p", "-auto-alt-ref", "0", "-b:v", "0", "-crf", "30", docs("demo.webm")]);
await ffmpeg([...frameInput, "-vf", `fps=${fps}`, "-c:v", "hevc_videotoolbox", "-pix_fmt", "bgra", "-alpha_quality", "0.9", "-q:v", "70", "-tag:v", "hvc1", "-movflags", "+faststart", docs("demo.mov")]);
// WebP frames come from the lossless captures, not the VP9 stream, so unchanged pixels
// really are unchanged and the encoder can skip them.
const webpFrames = join(work, "webp");
mkdirSync(webpFrames);
await ffmpeg([...frameInput, "-vf", `fps=${fps}`, join(webpFrames, "f%04d.png")]);
await new Promise((ok, reject) => {
    const files = readdirSync(webpFrames).sort().map((f) => join(webpFrames, f));
    const p = spawn("img2webp", ["-loop", "0", "-min_size", "-lossy", "-q", "75", "-m", "4", "-d", String(Math.round(1000 / fps)), ...files, "-o", github("demo.webp")], { stdio: ["ignore", "ignore", "inherit"] });
    p.on("exit", (code) => (code === 0 ? ok() : reject(new Error(`img2webp exited ${code}`))));
});

await stopChildren();
console.log(`${frames.length} frames, ${(frames.at(-1).t / 1000).toFixed(1)}s`);
process.exit(0);
