// The four manual lists, in the order they are shown.
// domains: hostlist entries; ips: IPv4/IPv6 addresses or subnets.
import type { Settings } from "../services/types";

export type ListKey = keyof Settings["userLists"];

export interface ManualList {
  key: ListKey;
  kind: "domain" | "ip";
  through: boolean;
  titleRu: string;
  titleEn: string;
  textRu: string;
  textEn: string;
  placeholder: string;
}

export const MANUAL_LISTS: ManualList[] = [
  {
    key: "general",
    kind: "domain",
    through: true,
    titleRu: "Сайты через обход",
    titleEn: "Sites via bypass",
    textRu: "Ваши домены, которые нужно открывать через обход. Наборы из каталога хранятся отдельно и здесь не показываются — удаление набора эти строки не трогает.",
    textEn: "Your domains to open through the bypass. Catalog sets are stored separately and are not shown here — removing a set never touches these lines.",
    placeholder: "example.com\n# комментарий",
  },
  {
    key: "ipsetInclude",
    kind: "ip",
    through: true,
    titleRu: "IP через обход",
    titleEn: "IPs via bypass",
    textRu: "IP-адреса и подсети, которые нужно пускать через обход (например серверы игры). Работают только в режимах ipset «Выбранные IP» и «Flowseal + выбранные» (Настройки → Обход).",
    textEn: "IP addresses and subnets to route through the bypass (e.g. game servers). Used only in the “Selected IPs” and “Flowseal + selected” ipset modes (Settings → Bypass).",
    placeholder: "149.154.160.0/20\n2001:db8::/32",
  },
  {
    key: "exclude",
    kind: "domain",
    through: false,
    titleRu: "Сайты — не трогать",
    titleEn: "Sites — leave alone",
    textRu: "Домены, которые обход никогда не затрагивает: банки, госуслуги, рабочие сервисы.",
    textEn: "Domains the bypass never touches: banking, government, work services.",
    placeholder: "bank.example",
  },
  {
    key: "ipsetExclude",
    kind: "ip",
    through: false,
    titleRu: "IP — не трогать",
    titleEn: "IPs — leave alone",
    textRu: "IP-адреса и подсети (например 192.168.0.0/16 — домашняя сеть), которые обход не затрагивает. Исключения важнее любых списков «через обход».",
    textEn: "IP addresses and subnets (e.g. 192.168.0.0/16 — your home network) the bypass leaves alone. Exclusions win over any “via bypass” list.",
    placeholder: "192.168.0.0/16\n10.0.0.1",
  },
];
