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

/*
Audio is decoded whole and played from memory (Web Audio), not streamed through an <audio>
element: WebKitGTK streams a blob through GStreamer, which played a long clip (a chapter) out of
order, starting with a stretch from its middle.
*/
let context: AudioContext | null = null;
let source: AudioBufferSourceNode | null = null;
let buffer: AudioBuffer | null = null;
/** The context's time when the audio's start would have played */
let startedAt = 0;
let frame = 0;

function now() {
  return context && buffer ? Math.min(context.currentTime - startedAt, buffer.duration) : 0;
}

function tick() {
  usePlayer.setState({ time: now() });
  if (source) frame = requestAnimationFrame(tick);
}

/** Plays `buffer` from `at` seconds */
function start(at: number) {
  if (!context || !buffer) return;
  if (source) {
    source.onended = null;
    source.stop();
  }
  const node = context.createBufferSource();
  node.buffer = buffer;
  node.connect(context.destination);
  node.onended = () => {
    if (source === node) stop();
  };
  startedAt = context.currentTime - at;
  node.start(0, Math.max(0, at));
  source = node;
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(tick);
}

export function stop() {
  if (source) {
    source.onended = null;
    source.stop();
    source = null;
  }
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
    context ??= new AudioContext();
    await context.resume();
    const bytes = await render();
    if (usePlayer.getState().loading !== id) return;
    const decoded = await context.decodeAudioData(bytes);
    if (usePlayer.getState().loading !== id) return;
    buffer = decoded;
    usePlayer.setState({ id, loading: null, offset, lead, time: 0, duration: decoded.duration });
    start(0);
  } catch (e) {
    usePlayer.setState({ loading: null, id: null });
    toast.error(`Couldn't play: ${errorMessage(e)}`);
  }
}

export function seek(seconds: number) {
  if (source && buffer) start(Math.min(Math.max(0, seconds), buffer.duration));
}

/** The sermon time being heard, if `id` is playing */
export function sourceTime(state: PlayerState, id: string): number | null {
  if (state.id !== id) return null;
  return state.offset + state.time - state.lead;
}
