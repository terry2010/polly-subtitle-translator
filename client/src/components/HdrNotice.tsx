// HDR/Dolby Vision 检测提示组件
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, X } from "lucide-react";
import { useVideoStore } from "../stores/videoStore";
import { HdrInfo } from "../lib/ipc-types";

export function HdrNotice() {
  const { t } = useTranslation();
  const { probeResult } = useVideoStore();
  const [dismissed, setDismissed] = useState(false);
  const [hdrInfo, setHdrInfo] = useState<HdrInfo | null>(null);

  useEffect(() => {
    setDismissed(false);
    const hdr = probeResult?.video_stream?.hdr_info ?? null;
    setHdrInfo(hdr);
  }, [probeResult]);

  if (!hdrInfo || dismissed) return null;

  return (
    <div className="flex items-center gap-2 rounded-md border border-orange-500/30 bg-orange-500/10 px-3 py-2 text-sm">
      <AlertTriangle className="h-4 w-4 flex-shrink-0 text-orange-600" />
      <div className="flex-1">
        <span className="font-medium text-orange-700">
          {hdrInfo.hdr_format}
        </span>
        <span className="ml-2 text-muted-foreground text-xs">
          {hdrInfo.details}
        </span>
        {hdrInfo.is_dolby_vision && (
          <span className="ml-2 text-xs text-orange-600">
            Dolby Vision 内容可能需要兼容播放器
          </span>
        )}
      </div>
      <button
        onClick={() => setDismissed(true)}
        className="text-muted-foreground hover:text-foreground"
      >
        <X className="h-3 w-3" />
      </button>
    </div>
  );
}
