/**
 * 设置页「自定义提供商」表单的草稿，以及两处需要判断「这份草稿要打给谁」的纯逻辑。
 *
 * 放在 lib 里是因为这两件事都会悄悄出错却看不出来：一个决定测试结果什么时候作废，
 * 另一个决定密钥留空到底是「清掉」还是「沿用」。留在组件里就没有测试盯着它们。
 */

/** 预设类型的标识，与 Rust 的 `CustomQuotaPreset` 一一对应。 */
export type CustomQuotaPreset =
  "openai_compatible" | "newapi" | "openrouter" | "deepseek" | "siliconflow" | "moonshot";

/** 表单草稿。`id` 为 null 表示新建；密钥留空 = 沿用已存的那把。 */
export type CustomQuotaDraft = {
  id: string | null;
  name: string;
  preset: CustomQuotaPreset;
  baseUrl: string;
  secret: string;
};

export const BLANK_CUSTOM_QUOTA_DRAFT: CustomQuotaDraft = {
  id: null,
  name: "",
  preset: "openai_compatible",
  baseUrl: "",
  secret: "",
};

/**
 * 草稿里**决定取数结果**的那三样，压成一个可比较的串。
 *
 * 「测试连接」的结果只在这个串没变时还算数：地址 / 类型 / 密钥一改，上一次读到的
 * 额度就是对着另一份配置打出来的，留在屏幕上等于给刚打上的新地址背书。
 * 同一个串也用来判断回显的地址还对不对得上输入框。
 *
 * 名称不在里面——改个名字不影响取数，没道理把刚测好的结果作废。比的也是**后端真正
 * 会用的那份值**：地址与密钥的首尾空格后端都会剃掉，为它作废结果只会让用户莫名其妙
 * 看着结果消失。密钥剃成空串正好与「留空 = 沿用已存的那把」重合，两者本就同义。
 */
export function fetchInputsOf(draft: CustomQuotaDraft): string {
  return JSON.stringify([draft.preset, draft.baseUrl.trim(), draft.secret.trim()]);
}

/**
 * 密钥框里的内容该怎么交给后端：留空是「沿用已存的那把」，因此空串必须变成 null。
 *
 * 交空串过去会被当成「用户要把密钥改成空的」——而界面上只有掩码，用户根本重打不出
 * 原文，编辑名称或地址时留空才是常态。
 */
export function submittedSecret(typed: string): string | null {
  return typed.trim() === "" ? null : typed;
}

/**
 * 某个预设在密钥框旁必须写明的令牌种类。没有特别要交代的就返回 null。
 *
 * NewAPI / OneAPI 走 OpenAI 兼容计费，钥匙是调模型的 `sk-` key。
 * 再写「系统访问令牌」会把用户往错误的钥匙上引。点数说明是因为站点后台
 * 若没开「以货币形式显示额度」，金额栏读到的是额度点数，百分比不受影响。
 */
export function credentialHint(preset: CustomQuotaPreset): string | null {
  if (preset === "newapi") {
    return "填调模型的 sk- key。站点后台若未开启「以货币形式显示额度」，金额一栏读到的是额度点数而不是美元；百分比不受影响。";
  }
  return null;
}

/**
 * 已实现预设的显示名，列成「甲」、「乙」。空列表与一档都不能冒出多余顿号。
 *
 * 设置页那句「暂未支持，现在只实现了…」和取数入口同一套拼法，新增档位时两边
 * 都从 `supported` / `implemented()` 推导，不再拼死某一档的名字。
 */
export function implementedPresetLabels(
  presets: ReadonlyArray<{ label: string; supported: boolean }>,
): string {
  return presets
    .filter((preset) => preset.supported)
    .map((preset) => `「${preset.label}」`)
    .join("、");
}

/** 密钥框的占位符：编辑时一律「留空不改」；新建一律提示 `sk-…`。 */
export function secretPlaceholder(_preset: CustomQuotaPreset, editing: boolean): string {
  if (editing) {
    return "不填就沿用现在这把";
  }
  return "sk-…";
}
