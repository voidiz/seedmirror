import { useMemo } from "react";
import type { FileSyncProgress } from "@/schemas/ws";
import { cn } from "@/lib/utils";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";

type SyncInfoFilenameProps = {
  item: FileSyncProgress;
  className?: string;
};

export function SyncInfoFilename({ item, className }: SyncInfoFilenameProps) {
  const displayFileName = useMemo(
    () => getCommonSuffix(item.remote_file_path, item.local_file_path),
    [item],
  );

  return (
    <Popover>
      <PopoverTrigger
        render={(props) => (
          <button
            className={cn(
              "text-left w-full truncate block underline decoration-muted-foreground/60 underline-offset-4 hover:decoration-foreground transition-all",
              className,
            )}
            {...props}
          >
            {displayFileName}
          </button>
        )}
      />
      <PopoverContent side="top" align="start" className="w-80 flex flex-col gap-4">
        <Path title="Local Path" path={item.local_file_path} />
        <Path title="Remote Path" path={item.remote_file_path} />
      </PopoverContent>
    </Popover>
  );
}

type PathProps = {
  title: string;
  path: string;
};

function Path({ title, path }: PathProps) {
  return (
    <div>
      <div className="text-sm font-semibold text-muted-foreground">{title}</div>
      <div className="text-xs font-mono break-all">{path}</div>
    </div>
  );
}

function getCommonSuffix(remotePath: string, localPath: string): string {
  if (!remotePath || !localPath) {
    return remotePath || localPath || "";
  }

  const partsRemote = remotePath.split("/").filter(Boolean);
  const partsLocal = localPath.split("/").filter(Boolean);

  const commonParts: string[] = [];
  let i = partsRemote.length - 1;
  let j = partsLocal.length - 1;

  while (i >= 0 && j >= 0 && partsRemote[i] === partsLocal[j]) {
    commonParts.unshift(partsRemote[i]);
    i--;
    j--;
  }

  if (commonParts.length === 0) {
    return partsRemote[partsRemote.length - 1] || remotePath;
  }

  return commonParts.join("/");
}
