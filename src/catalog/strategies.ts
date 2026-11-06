// Plain-language labels for strategy names (Flowseal names like
// `general (ALT3)` mean nothing to most people).

export type KindTone = "accent" | "ok" | "warn" | "neutral";
export interface StrategyKind {
  ru: string;
  en: string;
  hintRu: string;
  hintEn: string;
  tone: KindTone;
}

export function strategyKind(name: string, notes: string[] = []): StrategyKind {
  const n = name.toUpperCase();
  if (notes.some((x) => /not recommended/i.test(x)))
    return { ru: "не рекомендуется", en: "not recommended", hintRu: "Автор Flowseal не рекомендует эту стратегию.", hintEn: "The Flowseal author does not recommend this strategy.", tone: "warn" };
  if (/\bEXP\b/.test(n))
    return { ru: "экспериментальная", en: "experimental", hintRu: "Новые приёмы обхода, может работать нестабильно.", hintEn: "New techniques, may be unstable.", tone: "warn" };
  if (/FAKE TLS/.test(n))
    return { ru: "для проблемных сетей", en: "for tough networks", hintRu: "Подставляет поддельное TLS-приветствие. Пробуйте, если обычные не помогают.", hintEn: "Sends a fake TLS hello. Try when the usual ones fail.", tone: "accent" };
  if (/SIMPLE FAKE/.test(n))
    return { ru: "упрощённая", en: "simplified", hintRu: "Меньше приёмов — иногда стабильнее.", hintEn: "Fewer techniques — sometimes more stable.", tone: "neutral" };
  if (/\bALT\d*\b/.test(n))
    return { ru: "альтернативная", en: "alternative", hintRu: "Другие методы обхода, если базовая не работает.", hintEn: "Other methods, if the basic one does not work.", tone: "neutral" };
  if (/^GENERAL$/.test(n.trim()))
    return { ru: "базовая", en: "basic", hintRu: "С неё стоит начать.", hintEn: "Start with this one.", tone: "ok" };
  return { ru: "своя", en: "custom", hintRu: "Пользовательская стратегия.", hintEn: "User strategy.", tone: "neutral" };
}
