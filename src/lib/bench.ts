/**
 * Times the interface on real projects: run the app with `EAB_BENCH` set to project folders
 * (separated by `:`), and it opens each, goes through what's slow with thousands of sermons, prints
 * what it timed (JSON), and quits. Each step is timed from what's done until what it shows is painted,
 * with the longest frame meanwhile (how long the window stopped answering).
 *
 * Choosing readings and changing sermons' information are saved, so run it on copies of projects.
 */
import { api, errorMessage } from "./api";
import { useApp } from "./store";

interface Timing {
  project: string;
  step: string;
  ms: number;
  longestFrameMs: number;
  note?: string;
}

const frame = () => new Promise<number>((resolve) => requestAnimationFrame(resolve));
/** Once what's changed is drawn: React has committed it, and the frame after has painted */
const painted = async () => {
  await frame();
  await frame();
};

/** The longest gap between frames since `watch()` */
let worst = 0;
let last = 0;
const tick = (t: number) => {
  if (last) worst = Math.max(worst, t - last);
  last = t;
  requestAnimationFrame(tick);
};
const watch = () => {
  worst = 0;
  last = 0;
};

async function until(what: string, ready: () => boolean, timeoutMs = 300_000) {
  const start = performance.now();
  while (!ready()) {
    if (performance.now() - start > timeoutMs) throw new Error(`timed out waiting for ${what}`);
    await frame();
  }
}

const all = <T extends Element>(selector: string, root: ParentNode = document) => Array.from(root.querySelectorAll<T>(selector));
const withText = <T extends Element>(selector: string, text: RegExp, root: ParentNode = document) =>
  all<T>(selector, root).filter((e) => text.test(e.textContent ?? ""));
const button = (text: RegExp, root: ParentNode = document) => withText<HTMLButtonElement>("button", text, root)[0];
const spinning = (root: ParentNode = document) => root.querySelector(".animate-spin") !== null;

/** Types into an input as a person does (React reads its `input` events) */
function type(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

export async function runBench(folders: string[]) {
  requestAnimationFrame(tick);
  const only = await api.benchSteps();
  // The Sermons tab draws every sermon at once (minutes with tens of thousands), so it's timed only when asked for
  const wanted = (name: string) =>
    name.startsWith("open the project") ||
    (only.length === 0 && !name.startsWith("Sermons tab")) ||
    only.some((w) => name.toLowerCase().includes(w));
  const timings: Timing[] = [];
  let project = "";
  const step = async (name: string, action: () => unknown, done: () => boolean, note?: () => string) => {
    if (!wanted(name)) return;
    await stepTimed(name, action, done, note);
    const t = timings[timings.length - 1];
    void api.benchLog(`  ${t.ms} ms (longest frame ${t.longestFrameMs} ms)${t.note ? ` ${t.note}` : ""}`);
  };
  const stepTimed = async (name: string, action: () => unknown, done: () => boolean, note?: () => string) => {
    await painted();
    void api.benchLog(`… ${project}: ${name}`);
    watch();
    const start = performance.now();
    try {
      await action();
      await until(name, done);
      await painted();
      timings.push({ project, step: name, ms: Math.round(performance.now() - start), longestFrameMs: Math.round(worst), note: note?.() });
    } catch (e) {
      timings.push({ project, step: name, ms: -1, longestFrameMs: Math.round(worst), note: `failed: ${errorMessage(e)}` });
    }
  };
  const app = () => useApp.getState();

  for (const folder of folders) {
    project = folder.split("/").pop() ?? folder;
    let id = "";
    await step(
      "open the project (cold)",
      async () => {
        id = (await api.openProjectFolder(folder)).id;
        await app().refreshLists();
        await app().openProject(id);
      },
      () => {
        const s = app();
        if (!s.project) return false;
        if (s.project.kind === "readings") return withText("span", /readings of/).length > 0;
        return s.book !== null && s.chapter !== null && s.loadedChapters.has(`${s.book}.${s.chapter}`) && !!s.bookText[s.book];
      },
      () => `${app().project?.sources.length.toLocaleString()} sermons, ${app().summary.readings.toLocaleString()} readings`,
    );
    const readings = app().project?.kind === "readings";

    if (readings) {
      const sections = () => all("section").length;
      await step(
        "Readings: open the first sermon",
        () => all<HTMLButtonElement>("section button")[0]?.click(),
        () => all('[title="Open the transcript here"]').length > 0 && !spinning(),
        () => `${all('[title="Open the transcript here"]').length} readings shown`,
      );
      const before = sections();
      await step(
        "Readings: show 200 more sermons",
        () => button(/more sermons/)?.click(),
        () => sections() > before,
        () => `${sections()} sermons listed`,
      );
      await step(
        "Readings: by verse",
        () => button(/^By verse$/)?.click(),
        () => withText("button", /^(Old|New) Testament/).length > 0,
      );
      await step(
        "Readings: open John",
        () => all<HTMLButtonElement>("button").find((b) => b.querySelector("span")?.textContent === "John")?.click(),
        () => all<HTMLButtonElement>("button").some((b) => b.querySelector("span")?.textContent === "John 3"),
      );
      await step(
        "Readings: open John 3",
        () => all<HTMLButtonElement>("button").find((b) => b.querySelector("span")?.textContent === "John 3")?.click(),
        () => all('[title*="Open the transcript here"]').length > 0 && !spinning(),
        () => `${all('[title*="Open the transcript here"]').length.toLocaleString()} readings shown`,
      );
      const filter = () => document.querySelector<HTMLInputElement>('input[placeholder^="Filter by reference"]')!;
      const shown = () => withText("span", /^[\d,]+ shown$/)[0]?.textContent ?? "";
      await step("Readings: filter by “romans 8”", () => type(filter(), "romans 8"), () => shown() !== "", shown);
      await step("Readings: clear the filter", () => type(filter(), ""), () => shown() === "");
    } else {
      await step(
        "Verses: go to John 3",
        () => app().go(43, 3),
        () => app().loadedChapters.has("43.3") && !!app().bookText[43],
        () => `${app().matchesByVerse.get("43.3.16")?.length ?? 0} readings of John 3:16`,
      );
      // The verse's panel (the navigator is an aside too)
      const panel = () => withText("aside", /Readings found/)[0];
      await step(
        "Verses: open John 3:16",
        () => app().setVerse(16),
        // Its readings' cards are drawn (each may still be reading its waveform)
        () => !!panel() && withText("h3", /3:16/, panel()!).length > 0 && all("button", panel()!).length > 20,
      );
      await step(
        "Verses: show more readings of John 3:16",
        () => button(/^Show [\d,]+ more/, panel()!)?.click(),
        () => true,
      );
      await step(
        "Verses: go to Psalm 119",
        () => app().go(19, 119),
        () => app().loadedChapters.has("19.119") && !!app().bookText[19],
      );
      const start = performance.now();
      let longest = 0;
      for (let c = 1; c <= (wanted("Verses: every chapter of John") ? 21 : 0); c++) {
        watch();
        app().go(43, c);
        await until(`John ${c}`, () => app().loadedChapters.has(`43.${c}`) && !!app().bookText[43]);
        await painted();
        longest = Math.max(longest, worst);
      }
      if (wanted("Verses: every chapter of John")) timings.push({
        project,
        step: "Verses: every chapter of John, one after another",
        ms: Math.round(performance.now() - start),
        longestFrameMs: Math.round(longest),
        note: `${Math.round((performance.now() - start) / 21)} ms a chapter`,
      });
      app().go(43, 3);
      await until("John 3", () => app().loadedChapters.has("43.3"));
      await step(
        "Autosave: choose a reading of John 3:16",
        async () => {
          const best = app().matchesByVerse.get("43.3.16")?.[0];
          if (!best) throw new Error("John 3:16 has no readings");
          app().select(best);
          await app().flush();
        },
        () => !app().saving,
      );
      await step("Autosave: undo it", async () => {
        app().undo();
        await app().flush();
      }, () => !app().saving);
    }

    // A sermon with readings, for its transcript, and where its last one is
    const sermons = app().project?.sources ?? [];
    let sermon = "";
    let lastReading: { at: number; word: number } | null = null;
    for (const s of sermons.slice(0, 50)) {
      const found = (await api.sermonMatches(id, s.id)).matches;
      if (found.length > 0) {
        sermon = s.id;
        const last = found[found.length - 1];
        lastReading = { at: last.start, word: last.wordStart };
        break;
      }
    }
    const dialog = () => document.querySelector('[role="dialog"]');
    await step(
      "Transcript: open a sermon's",
      () => app().openTranscript(sermon),
      () => !!dialog() && !spinning(dialog()!) && (dialog()!.textContent?.length ?? 0) > 1000,
    );
    await step("Transcript: close it", () => app().openTranscript(null), () => !dialog());
    // Opened at a place late in it, that place is drawn and scrolled to
    await step(
      "Transcript: open it at its last reading",
      () => app().openTranscript(sermon, lastReading?.at ?? 0),
      () => {
        const word = dialog()?.querySelector(`[data-word="${lastReading?.word}"]`);
        const box = word?.closest(".overflow-y-auto")?.getBoundingClientRect();
        const at = word?.getBoundingClientRect();
        return !!box && !!at && at.top >= box.top && at.bottom <= box.bottom;
      },
    );
    await step("Transcript: close it again", () => app().openTranscript(null), () => !dialog());

    const fileId = sermons[0]?.fileId ?? sermons[0]?.label ?? "";
    await step("Autosave: change a sermon's speaker", async () => {
      app().setFileMetadata({ [fileId]: { speaker: "Benchmark" } });
      await app().flush();
    }, () => !app().saving);
    await step("Autosave: undo it", async () => {
      app().undo();
      await app().flush();
    }, () => !app().saving);

    const found = () => withText("div", /place(s)? in [\d,]+ sermon|wasn't found/).length > 0;
    await step(
      "Search: “the love of god”, until the first places show",
      async () => {
        app().setTab("search");
        await until("the search box", () => !!document.querySelector('input[placeholder^="Search what was said"]'));
        type(document.querySelector<HTMLInputElement>('input[placeholder^="Search what was said"]')!, "the love of god");
      },
      () => all("mark").length > 0 || found(),
      () => withText("div", /^Searching…/).pop()?.textContent ?? "",
    );
    await step(
      "Search: “the love of god”, until it's done",
      () => undefined,
      () => found() && !spinning(),
      () => withText("div", /place(s)? in [\d,]+ sermon/).pop()?.textContent?.slice(0, 80) ?? "",
    );

    await step(
      "Sermons tab",
      () => app().setTab("sources"),
      // Drawn as far as the screen shows (the rest as it's scrolled to)
      () => all("tbody tr").length > 5,
      () => `${all("tbody tr").length.toLocaleString()} rows`,
    );
    await step("Sermons tab: autosave a sermon's speaker", async () => {
      app().setFileMetadata({ [fileId]: { speaker: "Benchmark" } });
      await app().flush();
    }, () => !app().saving);
    await step("Sermons tab: undo it", async () => {
      app().undo();
      await app().flush();
    }, () => !app().saving);

    app().closeProject();
    await until("Home", () => !app().project);
    await step(
      "open the project again (warm)",
      () => app().openProject(id),
      () => {
        const s = app();
        if (!s.project) return false;
        if (readings) return withText("span", /readings of/).length > 0;
        return s.book !== null && s.loadedChapters.has(`${s.book}.${s.chapter}`);
      },
    );
    app().closeProject();
    await until("Home", () => !app().project);
  }
  await api.benchDone(JSON.stringify(timings, null, 1));
}
