import {
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useSyncData } from "@/hooks/use-sync-data";
import { SyncInfoRow } from "./sync-info-row";
import { ErrorMessage } from "./error-message";
import { ConnectedStatus } from "./connected-status";

export function SyncInfoTable() {
  const { isConnected, liveProgresses, activeTransfers, errorMessage } = useSyncData();

  return (
    <div className="max-w-4xl bg-card w-full rounded-md p-4 m-4 border border-border shadow-sm">
      <h1 className="mb-4 text-xl">seedmirror</h1>
      {errorMessage && <ErrorMessage text={errorMessage} />}

      <Table>
        <TableCaption>
          <ConnectedStatus isConnected={isConnected} />
        </TableCaption>
        <TableHeader>
          <TableRow>
            <TableHead className="w-70">File</TableHead>
            <TableHead className="w-35">Progress</TableHead>
            <TableHead className="text-right">Transferred</TableHead>
            <TableHead className="text-right">Speed</TableHead>
            <TableHead className="text-right">Remaining</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {liveProgresses.length === 0 ? (
            <TableRow>
              <TableCell colSpan={5} className="text-center text-muted-foreground py-8">
                No active file syncs in progress.
              </TableCell>
            </TableRow>
          ) : (
            liveProgresses.map((item) => <SyncInfoRow key={item.remote_file_path} item={item} />)
          )}
        </TableBody>
        <TableFooter>
          <TableRow className="text-muted-foreground">
            <TableCell colSpan={4}>Active Transfers</TableCell>
            <TableCell className="text-right">{activeTransfers}</TableCell>
          </TableRow>
        </TableFooter>
      </Table>
    </div>
  );
}
