import { TableRow, TableCell } from "./ui/table";
import { Progress } from "./ui/progress";
import { SyncInfoFilename } from "./sync-info-filename";
import type { FileSyncProgress } from "@/schemas/ws";

type SyncInfoRowProps = {
  item: FileSyncProgress;
};

export function SyncInfoRow({ item }: SyncInfoRowProps) {
  return (
    <TableRow key={`${item.remote_file_path}-${item.local_file_path}`}>
      <TableCell className="font-medium max-w-36">
        <SyncInfoFilename item={item} />
      </TableCell>

      <TableCell className="min-w-35">
        <div className="flex justify-center">
          <Progress value={item.progress} className="w-full" />
        </div>
      </TableCell>

      <TableCell className="text-right">{item.transferred}</TableCell>
      <TableCell className="text-right">{item.transfer_speed}</TableCell>
      <TableCell className="text-right font-mono">{item.remaining}</TableCell>
    </TableRow>
  );
}
