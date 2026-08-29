import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type Lang = "en" | "zh";

const STORAGE_KEY = "skillsync.lang";

function detectLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "zh") return saved;
    if (navigator.language.toLowerCase().startsWith("zh")) return "zh";
  } catch {
    /* storage unavailable — fall through to default */
  }
  return "en";
}

/**
 * UI strings. Backend-provided text (tool detection evidence, reload
 * guidance details, doctor details, native error messages) stays in
 * English; everything rendered as UI chrome is translated here.
 */
const messages = {
  en: {
    "nav.skills": "Skills",
    "nav.tools": "Tools",
    "nav.settings": "Settings",
    "app.tagline": "Local-first skill management. One canonical store, every tool in sync.",
    "app.autoSyncNote": "Auto-synced: {summary}",

    "common.refresh": "Refresh",
    "common.cancel": "Cancel",
    "common.close": "Close",
    "common.loading": "Loading…",

    "errors.nativeFailed": "Native operation failed",

    "skills.title": "Skills",
    "skills.count": "{count} skill(s) · canonical store {root}",
    "skills.canonicalMissing": " (not created yet)",
    "skills.searchPlaceholder": "Search skills…",
    "skills.ariaSearch": "Search skills",
    "skills.ariaStatusFilter": "Status filter",
    "skills.ariaToolFilter": "Tool filter",
    "skills.filters.all": "All",
    "skills.filters.synced": "Synced",
    "skills.filters.native": "Native",
    "skills.filters.unmanaged": "Unmanaged",
    "skills.filters.notInstalled": "Not installed",
    "skills.filters.conflict": "Conflict",
    "skills.filters.unavailable": "Unavailable",
    "skills.allTools": "All tools",
    "skills.emptyNone":
      "No skills discovered yet. Install tools or adopt skills into the canonical store first.",
    "skills.emptyFiltered": "No skills match the current filters.",
    "skills.badgeCanonical": "canonical",
    "skills.badgeUnmanaged": "unmanaged",
    "skills.details": "Details",
    "skills.hideDetails": "Hide details",
    "skills.fingerprint": "Fingerprint: ",
    "skills.installations": "Installations:",
    "skills.locations": "Locations",
    "skills.openCanonical": "Open canonical directory in file explorer",
    "skills.openToolDir": "Open {tool} directory in file explorer",
    "skills.open": "Open",
    "skills.preview": "Preview SKILL.md",
    "skills.hidePreview": "Hide SKILL.md",
    "skills.previewNote": "Read-only preview — external editors remain first-class.",
    "skills.importToStore": "Import to canonical store",
    "skills.planning": "Planning…",
    "skills.importPlan": "Import plan:",
    "skills.confirmImport": "Confirm import",
    "skills.keepBoth": "Keep both",
    "skills.replaceBackup": "Replace (backup first)",
    "skills.importedTo": "Imported to {path}",
    "skills.importedBackup": "Imported (previous copy backed up to {path})",

    "state.native": "Native",
    "state.synced": "Synced",
    "state.notInstalled": "Not installed",
    "state.disabled": "Disabled",
    "state.modified": "Modified",
    "state.conflict": "Conflict",
    "state.unmanaged": "Unmanaged",
    "state.unavailable": "Unavailable",

    "conflicts.title": "Conflicts need your decision",
    "conflicts.body":
      "A canonical skill and an unmanaged copy with the same name hold different content. Nothing is changed until you choose; whatever is replaced is backed up first.",
    "conflicts.compare": "Compare",
    "conflicts.hideDiff": "Hide diff",
    "conflicts.useCanonical": "Use canonical",
    "conflicts.keepBoth": "Keep both",
    "conflicts.importVersion": "Import {tool} version",
    "conflicts.ignore": "Ignore",
    "conflicts.canonical": "Canonical:",
    "conflicts.target": "Target:",
    "conflicts.noDifferences": "No file-level differences.",

    "firstImport.title": "Adopt your existing skills",
    "firstImport.unique": "{count} unique",
    "firstImport.duplicates": "{count} duplicate file(s)",
    "firstImport.conflictsBadge": "{count} conflict(s)",
    "firstImport.importN": "Import {count} skill(s)",
    "firstImport.importing": "Importing…",
    "firstImport.body":
      "Importing copies skills into {root} — your tool directories stay untouched. Conflicts (same name, different content) are never merged automatically.",
    "firstImport.from": "from {tool} · {path}",
    "firstImport.conflictLine":
      "{name}: {count} different versions — resolve manually after adopting the rest",
    "firstImport.doneTitle": "Imported {count} skill(s) into the canonical store.",
    "firstImport.doneBody":
      "Tool directories were not modified. Run a sync to install them into your tools as managed links or copies.",
    "firstImport.skipped": "skipped {name}: {reason}",
    "firstImport.failed": "failed {name}: {error}",

    "tools.title": "Tools",
    "tools.subtitle": "Detected AI coding tools and their skill locations.",
    "tools.detected": "Detected",
    "tools.notDetected": "Not detected",
    "tools.integration": "Integration",
    "tools.canonicalStore": "canonical store",
    "tools.override": "override",
    "tools.dirMissing": "directory missing",
    "tools.discovered": "Skills discovered:",
    "tools.managedSuffix": " ({managed} managed)",
    "tools.symlinks": "Symlinks:",
    "tools.reload": "Reload:",
    "tools.syncFromStore": "Sync from canonical store",
    "tools.syncPlan": "Sync plan — {method} into {dir}",
    "tools.storeEmpty":
      "Canonical store is empty or missing — nothing to sync. Adopt skills first.",
    "tools.apply": "Apply ({count} changes)",
    "tools.dryRun": "Dry run",
    "tools.dryRunPrefix": "Dry run — ",
    "tools.syncReport": "Sync {tool}: {summary}",

    "symlink.preferred": "preferred",
    "symlink.supported": "supported",
    "symlink.avoided": "avoided",

    "settings.title": "Settings",
    "settings.configNote":
      "Configuration lives in ~/.skillsync/config.json. Skill files are never stored here.",
    "settings.canonicalRoot": "Canonical skill root",
    "settings.canonicalRootHelp":
      "The single source of truth for your skills. May start with ~. It can be a plain folder or a git repository.",
    "settings.syncMethod": "Sync method",
    "settings.syncMethod.auto": "auto — link where safe, copy otherwise",
    "settings.syncMethod.symlink": "symlink — single physical copy",
    "settings.syncMethod.copy": "copy — fingerprint-tracked copies",
    "settings.syncMethodHelp": "Applies when skills are synced into tools.",
    "settings.autoSync": "Automatic synchronization",
    "settings.autoSyncHelp":
      "Watch the canonical store and refresh managed copies after changes (debounced). Symlinked targets need no copying. Off by default; manual Sync Now always stays available.",
    "settings.save": "Save settings",
    "settings.saving": "Saving…",
    "settings.saved": "Saved.",
    "settings.createFolder": "Create canonical folder",
    "settings.creating": "Creating…",
    "settings.ready": "Ready: {path}",
    "settings.language": "Language / 语言",
    "settings.diagnostics": "Diagnostics",
    "settings.diagnosticsHelp": "Environment checks shared with skillsync doctor.",
    "settings.runDoctor": "Run doctor",
    "settings.running": "Running…",
    "settings.runToInspect": "Run the check to inspect this machine.",

    "git.title": "Git repository (machine sync)",
    "git.description":
      "The canonical store can live in git. Every action here is explicit — nothing is ever committed, pulled or pushed automatically.",
    "git.notRepo": "not a git repository",
    "git.noUpstream": " · no upstream",
    "git.workingTreeClean": "Working tree clean.",
    "git.files": "{count} file(s)",
    "git.pull": "Pull (ff-only)",
    "git.pulling": "Pulling…",
    "git.commitMessage": "Commit message",
    "git.commitAll": "Commit all changes",
    "git.committing": "Committing…",
    "git.push": "Push",
    "git.pushing": "Pushing…",
    "git.ffNote":
      "Pull uses --ff-only; it never merges or overwrites local changes silently.",
    "git.initHint":
      "Run git init in the canonical store (or point it at a clone) to enable machine sync.",
  },

  zh: {
    "nav.skills": "技能",
    "nav.tools": "工具",
    "nav.settings": "设置",
    "app.tagline": "本地优先的技能管理：一份规范仓库，所有工具同步。",
    "app.autoSyncNote": "自动同步完成：{summary}",

    "common.refresh": "刷新",
    "common.cancel": "取消",
    "common.close": "关闭",
    "common.loading": "加载中…",

    "errors.nativeFailed": "原生操作失败",

    "skills.title": "技能",
    "skills.count": "{count} 个技能 · 规范仓库 {root}",
    "skills.canonicalMissing": "（尚未创建）",
    "skills.searchPlaceholder": "搜索技能…",
    "skills.ariaSearch": "搜索技能",
    "skills.ariaStatusFilter": "状态筛选",
    "skills.ariaToolFilter": "工具筛选",
    "skills.filters.all": "全部",
    "skills.filters.synced": "已同步",
    "skills.filters.native": "原生",
    "skills.filters.unmanaged": "未纳管",
    "skills.filters.notInstalled": "未安装",
    "skills.filters.conflict": "冲突",
    "skills.filters.unavailable": "不可用",
    "skills.allTools": "全部工具",
    "skills.emptyNone": "尚未发现技能。请先安装工具，或将技能导入规范仓库。",
    "skills.emptyFiltered": "没有符合当前筛选条件的技能。",
    "skills.badgeCanonical": "规范",
    "skills.badgeUnmanaged": "未纳管",
    "skills.details": "详情",
    "skills.hideDetails": "收起详情",
    "skills.fingerprint": "指纹：",
    "skills.installations": "安装位置：",
    "skills.locations": "位置",
    "skills.openCanonical": "在文件管理器中打开规范目录",
    "skills.openToolDir": "在文件管理器中打开 {tool} 目录",
    "skills.open": "打开",
    "skills.preview": "预览 SKILL.md",
    "skills.hidePreview": "收起 SKILL.md",
    "skills.previewNote": "只读预览——外部编辑器仍是首选。",
    "skills.importToStore": "导入规范仓库",
    "skills.planning": "规划中…",
    "skills.importPlan": "导入计划：",
    "skills.confirmImport": "确认导入",
    "skills.keepBoth": "两者保留",
    "skills.replaceBackup": "替换（先备份）",
    "skills.importedTo": "已导入到 {path}",
    "skills.importedBackup": "已导入（原副本备份于 {path}）",

    "state.native": "原生",
    "state.synced": "已同步",
    "state.notInstalled": "未安装",
    "state.disabled": "已停用",
    "state.modified": "已修改",
    "state.conflict": "冲突",
    "state.unmanaged": "未纳管",
    "state.unavailable": "不可用",

    "conflicts.title": "冲突需要你决定",
    "conflicts.body":
      "规范仓库与同名未纳管副本的内容不一致。在你做出选择之前不会改动任何文件；被替换的一方会先备份。",
    "conflicts.compare": "对比",
    "conflicts.hideDiff": "收起差异",
    "conflicts.useCanonical": "采用规范版本",
    "conflicts.keepBoth": "两者保留",
    "conflicts.importVersion": "采用 {tool} 版本",
    "conflicts.ignore": "忽略",
    "conflicts.canonical": "规范：",
    "conflicts.target": "目标：",
    "conflicts.noDifferences": "没有文件级差异。",

    "firstImport.title": "收编现有技能",
    "firstImport.unique": "{count} 个待导入",
    "firstImport.duplicates": "{count} 处重复",
    "firstImport.conflictsBadge": "{count} 个冲突",
    "firstImport.importN": "导入 {count} 个技能",
    "firstImport.importing": "导入中…",
    "firstImport.body":
      "导入会把技能复制到 {root}——工具目录保持不变。冲突（同名不同内容）永远不会被自动合并。",
    "firstImport.from": "来自 {tool} · {path}",
    "firstImport.conflictLine":
      "{name}：{count} 个不同版本——导入其余技能后请手动处理",
    "firstImport.doneTitle": "已将 {count} 个技能导入规范仓库。",
    "firstImport.doneBody":
      "工具目录未被修改。运行同步即可将它们以托管链接或副本安装到各工具。",
    "firstImport.skipped": "已跳过 {name}：{reason}",
    "firstImport.failed": "失败 {name}：{error}",

    "tools.title": "工具",
    "tools.subtitle": "检测到的 AI 编码工具及其技能目录。",
    "tools.detected": "已检测",
    "tools.notDetected": "未检测到",
    "tools.integration": "集成",
    "tools.canonicalStore": "规范仓库",
    "tools.override": "覆盖",
    "tools.dirMissing": "目录不存在",
    "tools.discovered": "发现的技能：",
    "tools.managedSuffix": "（{managed} 个托管）",
    "tools.symlinks": "符号链接：",
    "tools.reload": "重载：",
    "tools.syncFromStore": "从规范仓库同步",
    "tools.syncPlan": "同步计划 — 以 {method} 同步到 {dir}",
    "tools.storeEmpty": "规范仓库为空或不存在——没有可同步内容。请先导入技能。",
    "tools.apply": "应用（{count} 项变更）",
    "tools.dryRun": "预演",
    "tools.dryRunPrefix": "预演 — ",
    "tools.syncReport": "同步 {tool}：{summary}",

    "symlink.preferred": "优先链接",
    "symlink.supported": "可用",
    "symlink.avoided": "不可用",

    "settings.title": "设置",
    "settings.configNote":
      "配置保存在 ~/.skillsync/config.json。技能文件永远不会存储在这里。",
    "settings.canonicalRoot": "规范技能仓库",
    "settings.canonicalRootHelp":
      "技能的唯一事实来源。可以以 ~ 开头；可以是普通文件夹或 git 仓库。",
    "settings.syncMethod": "同步方式",
    "settings.syncMethod.auto": "auto — 安全处用链接，否则复制",
    "settings.syncMethod.symlink": "symlink — 单一物理副本",
    "settings.syncMethod.copy": "copy — 带指纹追踪的副本",
    "settings.syncMethodHelp": "将技能同步到工具时生效。",
    "settings.autoSync": "自动同步",
    "settings.autoSyncHelp":
      "监视规范仓库，变更后（防抖）刷新托管副本。符号链接目标无需复制。默认关闭；手动“立即同步”始终可用。",
    "settings.save": "保存设置",
    "settings.saving": "保存中…",
    "settings.saved": "已保存。",
    "settings.createFolder": "创建规范目录",
    "settings.creating": "创建中…",
    "settings.ready": "已就绪：{path}",
    "settings.language": "Language / 语言",
    "settings.diagnostics": "诊断",
    "settings.diagnosticsHelp": "与 skillsync doctor 共用的环境检查。",
    "settings.runDoctor": "运行诊断",
    "settings.running": "运行中…",
    "settings.runToInspect": "运行检查以查看本机状态。",

    "git.title": "Git 仓库（跨机器同步）",
    "git.description":
      "规范仓库可以放在 git 里。这里的每个操作都需手动触发——绝不自动 commit/pull/push。",
    "git.notRepo": "不是 git 仓库",
    "git.noUpstream": " · 无上游",
    "git.workingTreeClean": "工作区干净。",
    "git.files": "{count} 个文件",
    "git.pull": "拉取（ff-only）",
    "git.pulling": "拉取中…",
    "git.commitMessage": "提交信息",
    "git.commitAll": "提交全部变更",
    "git.committing": "提交中…",
    "git.push": "推送",
    "git.pushing": "推送中…",
    "git.ffNote": "拉取使用 --ff-only；绝不会静默合并或覆盖本地更改。",
    "git.initHint": "在规范仓库中执行 git init（或指向一个克隆）以启用跨机器同步。",
  },
} as const;

type MessageKey = keyof typeof messages.en;

type I18nValue = {
  lang: Lang;
  setLang: (lang: Lang) => void;
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
};

const I18nContext = createContext<I18nValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectLang);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, lang);
    } catch {
      /* ignore */
    }
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
  }, [lang]);

  const setLang = useCallback((next: Lang) => setLangState(next), []);

  const t = useCallback<I18nValue["t"]>(
    (key, vars) => {
      const table = messages[lang] as Record<string, string>;
      const fallback = messages.en as Record<string, string>;
      let text = table[key] ?? fallback[key] ?? key;
      if (vars) {
        for (const [name, value] of Object.entries(vars)) {
          text = text.replaceAll(`{${name}}`, String(value));
        }
      }
      return text;
    },
    [lang],
  );

  const value = useMemo(() => ({ lang, setLang, t }), [lang, setLang, t]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/**
 * English-only fallback so components remain usable outside the provider
 * (tests, storybook-style renderings). The app itself always mounts the
 * provider in main.tsx.
 */
const fallback: I18nValue = {
  lang: "en",
  setLang: () => {},
  t: (key, vars) => {
    let text = (messages.en as Record<string, string>)[key] ?? key;
    if (vars) {
      for (const [name, value] of Object.entries(vars)) {
        text = text.replaceAll(`{${name}}`, String(value));
      }
    }
    return text;
  },
};

export function useI18n(): I18nValue {
  return useContext(I18nContext) ?? fallback;
}
