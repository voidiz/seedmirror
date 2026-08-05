import { useMemo } from "react";
import type { FileSyncProgress } from "@/schemas/ws";
import { cn } from "@/lib/utils";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";

type SyncInfoFilenameProps = {
  item: FileSyncProgress;
  className?: string;
};

export function SyncInfoFilename({ item, className }: SyncInfoFilenameProps) {
  const displayFileName = useMemo(() => basename(item.remote_file_path), [item]);

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
        <Path title="Remote Path" path={item.remote_file_path} />
        <Path title="Local Path" path={item.local_file_path} />
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

function basename(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}
