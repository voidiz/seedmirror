import { useSyncData } from "@/hooks/use-sync-data";
import { SyncInfoFilename } from "./sync-info-filename";
import { Progress } from "./ui/progress";
import { ErrorMessage } from "./error-message";
import { ConnectedStatus } from "./connected-status";

export function SyncInfoList() {
  const { isConnected, liveProgresses, activeTransfers, errorMessage } = useSyncData();

  return (
    <div className="w-full h-full p-4">
      <h1 className="text-xl">seedmirror</h1>
      <p className="text-sm text-muted-foreground pb-4">Active Transfers: {activeTransfers}</p>

      {errorMessage && <ErrorMessage text={errorMessage} />}

      {liveProgresses.length === 0 && (
        <p className="text-muted-foreground text-sm text-center">
          No active file syncs in progress.
        </p>
      )}

      <ul className="list-none w-full flex flex-col gap-2">
        {liveProgresses.map((item) => (
          <li
            key={item.remote_file_path}
            className="bg-card rounded-sm border border-border p-4 flex flex-col gap-2"
          >
            <SyncInfoFilename className="font-bold text-sm" item={item} />
            <div className="text-xs text-muted-foreground flex gap-4 divide-x divide-red-400 pb-2">
              <span>{item.transferred}</span>
            </div>
            <div className="flex gap-4 items-center">
              <Progress value={item.progress} className="w-full" />
              {item.transfer_speed !== "-" && (
                <span className="text-xs text-muted-foreground">{item.transfer_speed}</span>
              )}
            </div>
            <div className="text-sm justify-end font-mono text-muted-foreground">
              {item.remaining}
            </div>
          </li>
        ))}
      </ul>
      <div className="p-4 text-sm">
        <ConnectedStatus isConnected={isConnected} />
      </div>
    </div>
  );
}
