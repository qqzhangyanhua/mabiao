import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import {
  fetchInputsOf,
  submittedSecret,
  type CustomQuotaDraft,
  type CustomQuotaPreset,
} from "../lib/customQuotaDraft";
import { formatClock, humanStatus } from "../lib/format";
import type { OfficialQuotaWindow } from "../types";
import { OfficialQuotaWindows } from "./OfficialQuotaPanel";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";

/**
 * 新增 / 编辑一条自定义提供商的表单，连同只有它用得到的两条命令：回显请求地址、
 * 测试连接。草稿本身与那两处纯判断住在 `lib/customQuotaDraft.ts`，好让它们有测试盯着。
 */
export type CustomQuotaPresetDto = {
  value: CustomQuotaPreset;
  label: string;
  /** 本版是否已有解析器。为 false 时界面给「暂未支持」提示。 */
  supported: boolean;
};

/** 取数时真正会请求的一条地址。归一化只在 Rust 存在一份，这里不重写。 */
type QuotaRequestDto = {
  url: string;
  /** 为 false 时这条拿不到只是少一个口径，不是取数失败。 */
  required: boolean;
};

type CustomQuotaRequestPreviewDto = {
  requests: QuotaRequestDto[];
  /** 地址还不成形（空着、少协议头、预设没实现）时的人话提示。 */
  error: string | null;
};

/** 测试连接的请求。名称不影响取数，因此不必先起好名字才能测。 */
type TestCustomQuotaProvider = {
  id: string | null;
  preset: CustomQuotaPreset;
  base_url: string;
  secret: string | null;
};

type CustomQuotaTestDto = {
  windows: OfficialQuotaWindow[];
  captured_at: string;
};

/** 防抖：边打边回显，但不必每敲一个字符都发一次命令。 */
const ECHO_DEBOUNCE_MS = 250;

/**
 * base URL 输入框下方那行回显，内容来自后端**取数时用的**那份归一化。
 *
 * 前端一个字都不重写：回显存在的唯一目的就是「点保存之前就知道填对没有」，
 * 两份实现哪天只改了一边，回显就开始骗人。
 *
 * 回来的地址连同它是**照哪份输入**算的一起收着：换了预设类型、或者防抖那 250 毫秒
 * 里又敲了几个字，上一条就对不上输入框了。此时宁可什么都不显示——一个属于上一份
 * 输入的地址比空着更糟，那正是这行回显要消灭的东西。
 */
function useRequestPreview(preset: CustomQuotaPreset, baseUrl: string) {
  const typed = baseUrl.trim();
  const asked = JSON.stringify([preset, typed]);
  const [preview, setPreview] = useState<{
    asked: string;
    dto: CustomQuotaRequestPreviewDto;
  } | null>(null);
  useEffect(() => {
    if (typed === "") {
      return;
    }
    // 防抖只挡住「还没打完就发命令」，挡不住「先发的后回来」：慢一拍的旧响应
    // 会盖掉刚回来的新地址。因此过期的那次直接丢掉。
    let current = true;
    const timer = window.setTimeout(() => {
      void invoke<CustomQuotaRequestPreviewDto>("preview_custom_quota_request", {
        preset,
        baseUrl: typed,
      })
        .then((dto) => {
          if (current) {
            setPreview({ asked, dto });
          }
        })
        .catch((cause: unknown) => {
          if (current) {
            setPreview({ asked, dto: { requests: [], error: humanStatus(cause) } });
          }
        });
    }, ECHO_DEBOUNCE_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [asked, preset, typed]);
  // 输入框空着就不画这一行。靠输入本身判断而不是去清状态：清空是渲染时就知道的事。
  return typed !== "" && preview?.asked === asked ? preview.dto : null;
}

/**
 * 一次测试的结果，连同它是**对着哪份取数输入**打出来的（`fetchInputsOf`）。
 * 读到额度与失败二选一，没有第三种形状。
 */
type TestOutcome = { inputs: string } & (
  { read: CustomQuotaTestDto; error?: undefined } | { read?: undefined; error: string }
);

export function CustomQuotaProviderForm({
  draft,
  presets,
  busy,
  onChange,
  onCancel,
  onSubmit,
}: {
  draft: CustomQuotaDraft;
  presets: CustomQuotaPresetDto[];
  busy: boolean;
  onChange: (draft: CustomQuotaDraft) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  const selected = presets.find((preset) => preset.value === draft.preset);
  const editing = draft.id != null;
  const preview = useRequestPreview(draft.preset, draft.baseUrl);
  const [testing, setTesting] = useState(false);
  const [outcome, setOutcome] = useState<TestOutcome | null>(null);
  // 结果跟着它当时测的那份输入走：地址 / 类型 / 密钥一改就自动不认了。
  // 否则那句「读到 $19」会留在屏幕上，给刚打上的新地址背书。
  const current = outcome?.inputs === fetchInputsOf(draft) ? outcome : null;

  async function test() {
    setTesting(true);
    setOutcome(null);
    const request: TestCustomQuotaProvider = {
      id: draft.id,
      preset: draft.preset,
      base_url: draft.baseUrl,
      secret: submittedSecret(draft.secret),
    };
    // 认的是点下按钮那一刻的输入：测试打到一半用户又改了地址，回来的结果
    // 不该挂到新地址上。
    const inputs = fetchInputsOf(draft);
    try {
      const read = await invoke<CustomQuotaTestDto>("test_custom_quota_provider", { request });
      setOutcome({ inputs, read });
    } catch (cause) {
      // 失败只写在这里，保存按钮一动不动：断网、中转站临时抽风、或者用户想先把
      // 配置填好稍后再验，都不该变成「存不进去」。
      setOutcome({ inputs, error: humanStatus(cause) });
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className="custom-quota-form">
      <Field
        label="名称"
        placeholder="例如：公司的中转"
        value={draft.name}
        onChange={(event) => onChange({ ...draft, name: event.target.value })}
      />
      <label className="field">
        <span className="field-label">预设类型</span>
        <select
          value={draft.preset}
          onChange={(event) =>
            onChange({ ...draft, preset: event.target.value as CustomQuotaPreset })
          }
        >
          {presets.map((preset) => (
            <option key={preset.value} value={preset.value}>
              {preset.supported ? preset.label : `${preset.label}（暂未支持）`}
            </option>
          ))}
        </select>
      </label>
      <Field
        label="base URL"
        placeholder="https://relay.example.com"
        value={draft.baseUrl}
        onChange={(event) => onChange({ ...draft, baseUrl: event.target.value })}
      />
      <RequestEcho preview={preview} />
      <Field
        label={editing ? "密钥（留空则不改）" : "密钥"}
        type="password"
        autoComplete="off"
        placeholder={editing ? "不填就沿用现在这把" : "sk-…"}
        value={draft.secret}
        onChange={(event) => onChange({ ...draft, secret: event.target.value })}
      />
      {selected && !selected.supported ? (
        <p className="panel-note tone-warn">
          「{selected.label}」暂未支持，现在只实现了「OpenAI 兼容计费」。
          可以先存着，取数时会明确告诉你还没实现。
        </p>
      ) : (
        <p className="panel-note">
          base URL 填根地址即可，带 <code>/v1</code> 或结尾斜杠都认。
        </p>
      )}
      <TestOutcomeNote outcome={current} />
      <div className="row-actions">
        <Button variant="accent" disabled={busy} onClick={onSubmit}>
          {busy ? "保存中…" : "保存"}
        </Button>
        {/* 测试与保存互不相干：测不通照样能存，没测过也照样能存。 */}
        <Button disabled={busy || testing} onClick={() => void test()}>
          {testing ? "测试中…" : "测试连接"}
        </Button>
        <Button disabled={busy} onClick={onCancel}>
          取消
        </Button>
      </div>
    </div>
  );
}

/** 「将请求：<完整地址>」。地址还不成形时给提示，而不是把半截地址念出来。 */
function RequestEcho({ preview }: { preview: CustomQuotaRequestPreviewDto | null }) {
  if (!preview) {
    return null;
  }
  if (preview.error) {
    return <p className="panel-note tone-warn custom-quota-echo">{preview.error}</p>;
  }
  return (
    <div className="custom-quota-echo">
      <span className="field-label">将请求</span>
      <ul>
        {preview.requests.map((request) => (
          <li key={request.url}>
            <code>{request.url}</code>
            {request.required ? null : (
              <span className="muted" title="这条拿不到就只显示金额，不算取数失败">
                可选
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * 测试结果。成功时把解析出的额度直接画出来——「成功」两个字证明不了预设类型
 * 选对了，读到的数才能。画法复用首页那一套，用户现在确认的就是之后每天看到的。
 */
function TestOutcomeNote({ outcome }: { outcome: TestOutcome | null }) {
  if (!outcome) {
    return null;
  }
  if (outcome.error !== undefined) {
    return <p className="panel-note tone-danger">测试失败：{outcome.error}（不影响保存）</p>;
  }
  return (
    <div className="custom-quota-test-result">
      <p className="panel-note tone-ok">读到（{formatClock(outcome.read.captured_at)}）：</p>
      <OfficialQuotaWindows windows={outcome.read.windows} />
    </div>
  );
}
