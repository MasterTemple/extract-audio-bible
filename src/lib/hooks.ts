import { useEffect, useState } from "react";
import { api, type Word } from "./api";

const wordCache = new Map<string, Promise<{ from: number; words: Word[] }>>();

/** Words `from`..`to` (inclusive) of a transcript */
export function useWords(transcriptPath: string | undefined, from: number, to: number) {
  const [data, setData] = useState<{ from: number; words: Word[] } | null>(null);
  useEffect(() => {
    if (!transcriptPath) return;
    const key = `${transcriptPath}:${from}:${to}`;
    let promise = wordCache.get(key);
    if (!promise) {
      promise = api.transcriptWords(transcriptPath, from, to);
      wordCache.set(key, promise);
      promise.catch(() => wordCache.delete(key));
    }
    let live = true;
    promise.then((d) => live && setData(d)).catch(() => undefined);
    return () => {
      live = false;
    };
  }, [transcriptPath, from, to]);
  return data;
}

type Peaks = Awaited<ReturnType<typeof api.audioPeaks>>;

/** The whole file's peaks are read once (the backend caches them); windows are cut from those */
export function usePeaks(audioPath: string | undefined, start: number, end: number) {
  const [data, setData] = useState<Peaks | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!audioPath) return;
    let live = true;
    setError(null);
    api
      .audioPeaks(audioPath, start, end)
      .then((d) => live && setData(d))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [audioPath, start, end]);
  return { data, error };
}
