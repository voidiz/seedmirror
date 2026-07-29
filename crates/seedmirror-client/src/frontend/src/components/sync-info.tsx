import { useMediaQuery } from "@/hooks/use-media-query";
import { SyncInfoTable } from "./sync-info-table";
import { SyncInfoList } from "./sync-info-list";

export function SyncInfo() {
  const isMobile = useMediaQuery("(width <= 767px)");

  if (isMobile) {
    return <SyncInfoList />;
  }

  return <SyncInfoTable />;
}
