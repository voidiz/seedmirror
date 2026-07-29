import { cn } from "@/lib/utils";

export function ConnectedStatus({
  isConnected,
  className,
}: {
  isConnected: boolean;
  className?: string;
}) {
  return (
    <span className={cn("flex items-center justify-center gap-2 text-muted-foreground", className)}>
      {isConnected ? (
        <>
          <span className="h-2 w-2 rounded-full bg-emerald-600" />
          Connected
        </>
      ) : (
        <>
          <span className="h-2 w-2 rounded-full bg-destructive" />
          Disconnected from seedmirror-client. Reconnecting...
        </>
      )}
    </span>
  );
}
