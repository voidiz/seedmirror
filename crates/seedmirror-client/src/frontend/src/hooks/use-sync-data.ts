import { useCallback, useEffect, useMemo, useState } from "react";

import { type FileSyncProgress } from "@/schemas/ws";
import { defaultWsClient as wsClient, type WsStatus } from "@/data/ws";

export function useSyncData() {
  const [status, setStatus] = useState<WsStatus>(wsClient.status);
  const [progressMap, setProgressMap] = useState<Map<string, FileSyncProgress>>(new Map());
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const getCurrentStatus = useCallback(async () => {
    const result = await wsClient.request({ type: "get_current_status" }).catch((err) => {
      setErrorMessage(err.toString());
    });

    if (!result) {
      return;
    }

    const initialMap = new Map<string, FileSyncProgress>();
    result.data.forEach((item) => initialMap.set(item.remote_file_path, item));
    setProgressMap(initialMap);
  }, []);

  const liveProgresses = useMemo(
    () => Array.from(progressMap.values()).toReversed(),
    [progressMap],
  );

  const activeTransfers = useMemo(
    () => liveProgresses.reduce((acc, val) => (val.progress === 100 ? acc : acc + 1), 0),
    [liveProgresses],
  );

  useEffect(() => {
    const unsubscribeStatus = wsClient.onStatusChange(setStatus);

    const unsubscribeMessages = wsClient.onMessage((msg) => {
      switch (msg.type) {
        case "current_status": {
          const initialMap = new Map<string, FileSyncProgress>();
          msg.data.forEach((item) => initialMap.set(item.remote_file_path, item));
          setProgressMap(initialMap);
          break;
        }

        case "sync_progress": {
          setProgressMap((prev) => new Map(prev).set(msg.data.remote_file_path, msg.data));
          break;
        }

        case "error": {
          setErrorMessage(`${msg.data.code}: ${msg.data.message}`);
          break;
        }
      }
    });

    const unsubscribeErrors = wsClient.onError((err) => {
      console.error(`Websocket error:`, err);
    });

    if (wsClient.connect()) {
      getCurrentStatus();
    }

    return () => {
      unsubscribeStatus();
      unsubscribeMessages();
      unsubscribeErrors();
    };
  }, [getCurrentStatus]);

  return {
    isConnected: status === "CONNECTED",
    status,
    liveProgresses,
    activeTransfers,
    errorMessage,
  };
}
