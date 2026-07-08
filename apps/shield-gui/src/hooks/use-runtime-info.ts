import { useCallback, useEffect, useState } from "react";
import { api, type BuildInfo } from "@/lib/tauri";

export function useRuntimeInfo() {
  const [buildInfo, setBuildInfo] = useState<BuildInfo | null>(null);
  const [runtimeInfoLoaded, setRuntimeInfoLoaded] = useState(false);
  const [runtimeInfoRefreshing, setRuntimeInfoRefreshing] = useState(false);

  const refreshRuntimeInfo = useCallback(async () => {
    setRuntimeInfoRefreshing(true);
    try {
      const info = await api.getBuildInfo();
      setBuildInfo(info);
    } finally {
      setRuntimeInfoLoaded(true);
      setRuntimeInfoRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void refreshRuntimeInfo();
  }, [refreshRuntimeInfo]);

  return {
    buildInfo,
    runtimeInfoLoaded,
    runtimeInfoRefreshing,
    refreshRuntimeInfo,
  };
}
