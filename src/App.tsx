import { useEffect, useState } from "react";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Home } from "@/components/Home";
import { Shell } from "@/components/Shell";
import { api } from "@/lib/api";
import { lastProjectId, useApp } from "@/lib/store";

export default function App() {
  const project = useApp((s) => s.project);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const start = async () => {
      // With project folders to time it on (`EAB_BENCH`), it runs its benchmark instead
      const bench = await api.benchFolders();
      if (bench.length > 0) {
        setReady(true);
        const { runBench } = await import("@/lib/bench");
        await runBench(bench);
        return;
      }
      await useApp.getState().refreshLists();
      const last = lastProjectId();
      if (last && useApp.getState().projects.some((p) => p.id === last)) {
        await useApp.getState().openProject(last).catch(() => undefined);
      }
      setReady(true);
    };
    void start();
    const flush = () => void useApp.getState().flush();
    window.addEventListener("beforeunload", flush);
    return () => window.removeEventListener("beforeunload", flush);
  }, []);

  return (
    <TooltipProvider delayDuration={300}>
      {ready && (project ? <Shell /> : <Home />)}
      <Toaster richColors position="bottom-right" />
    </TooltipProvider>
  );
}
