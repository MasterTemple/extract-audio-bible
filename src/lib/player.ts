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
  /** Stretches of the sermon left out of the audio (cut out of a reading), in order */
  skips: [number, number][];
  /** Seconds into the audio */
  time: number;
  duration: number;
  /** Stopped where it is, to carry on from there (`resume`) */
  paused: boolean;
}

export const usePlayer = create<PlayerState>(() => ({
  id: null,
  loading: null,
  offset: 0,
  lead: 0,
  skips: [],
  time: 0,
  duration: 0,
  paused: false,
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
/** Where the audio was paused, while it is */
let pausedAt: number | null = null;
let frame = 0;

function now() {
  if (pausedAt !== null) return pausedAt;
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
  pausedAt = null;
  usePlayer.setState({ paused: false, time: at });
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(tick);
}

/** Stops the sound, keeping what's playing (to pause it) */
function silence() {
  if (source) {
    source.onended = null;
    source.stop();
    source = null;
  }
  cancelAnimationFrame(frame);
}

export function stop() {
  silence();
  pausedAt = null;
  usePlayer.setState({ id: null, loading: null, time: 0, paused: false });
}

/** Stops where it is, to carry on from there */
export function pause() {
  if (!source || !buffer) return;
  const at = now();
  silence();
  pausedAt = at;
  usePlayer.setState({ paused: true, time: at });
}

/** Carries on from where it was paused */
export function resume() {
  if (pausedAt === null || !buffer || !usePlayer.getState().id) return;
  void context?.resume();
  start(pausedAt);
}

/** Pauses what's playing, or carries on with what's paused */
export function togglePause() {
  if (usePlayer.getState().paused) resume();
  else pause();
}

/**
 * Play WAV bytes, rendered on demand; `offset` maps the audio's time back to the sermon's, past
 * the stretches it leaves out (`skips`). From `at` seconds into it, if given. Playing what's
 * playing (or paused) stops it
 */
export async function play(id: string, render: () => Promise<ArrayBuffer>, offset = 0, lead = 0, at = 0, skips: [number, number][] = []) {
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
    usePlayer.setState({ id, loading: null, offset, lead, skips: [...skips].sort((a, b) => a[0] - b[0]), time: 0, duration: decoded.duration });
    start(Math.min(Math.max(0, at), decoded.duration));
  } catch (e) {
    usePlayer.setState({ loading: null, id: null });
    toast.error(`Couldn't play: ${errorMessage(e)}`);
  }
}

/** Goes to a time in what's playing, or what's paused (staying paused) */
export function seek(seconds: number) {
  if (!buffer || !usePlayer.getState().id) return;
  const at = Math.min(Math.max(0, seconds), buffer.duration);
  if (pausedAt !== null) {
    pausedAt = at;
    usePlayer.setState({ time: at });
  } else if (source) start(at);
}

/** The sermon time being heard, if `id` is playing */
export function sourceTime(state: PlayerState, id: string): number | null {
  if (state.id !== id) return null;
  let at = state.offset + state.time - state.lead;
  for (const [from, to] of state.skips) if (at >= from) at += to - from;
  return at;
}
