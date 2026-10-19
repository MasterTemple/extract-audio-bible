import { create } from "zustand";
import { toast } from "sonner";
import { errorMessage } from "./api";

interface PlayerState {
  /** What's playing (a match id, "chapter:43.3", ...) */
  id: string | null;
  loading: string | null;
  /** The sermon's time at the start of the audio, after `lead` seconds of added silence */
  offset: number;
  lead: number;
  /** Seconds into the audio */
  time: number;
  duration: number;
}

export const usePlayer = create<PlayerState>(() => ({
  id: null,
  loading: null,
  offset: 0,
  lead: 0,
  time: 0,
  duration: 0,
}));

const audio = new Audio();
let url: string | null = null;
let frame = 0;

function tick() {
  usePlayer.setState({ time: audio.currentTime });
  if (!audio.paused) frame = requestAnimationFrame(tick);
}

audio.addEventListener("play", () => {
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(tick);
});
audio.addEventListener("ended", () => stop());
audio.addEventListener("loadedmetadata", () => usePlayer.setState({ duration: audio.duration }));

export function stop() {
  audio.pause();
  cancelAnimationFrame(frame);
  usePlayer.setState({ id: null, loading: null, time: 0 });
}

/** Play WAV bytes, rendered on demand; `offset` maps the audio's time back to the sermon's */
export async function play(id: string, render: () => Promise<ArrayBuffer>, offset = 0, lead = 0) {
  if (usePlayer.getState().id === id) {
    stop();
    return;
  }
  stop();
  usePlayer.setState({ loading: id });
  try {
    const bytes = await render();
    if (usePlayer.getState().loading !== id) return;
    if (url) URL.revokeObjectURL(url);
    url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
    audio.src = url;
    usePlayer.setState({ id, loading: null, offset, lead, time: 0 });
    await audio.play();
  } catch (e) {
    usePlayer.setState({ loading: null, id: null });
    toast.error(`Couldn't play: ${errorMessage(e)}`);
  }
}

export function seek(seconds: number) {
  audio.currentTime = Math.max(0, seconds);
}

/** The sermon time being heard, if `id` is playing */
export function sourceTime(state: PlayerState, id: string): number | null {
  if (state.id !== id) return null;
  return state.offset + state.time - state.lead;
}
