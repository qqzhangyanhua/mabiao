import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { ConversationPage, Filter } from "../types";
import { CATALOG_PAGE_SIZE } from "./conversationCatalogItems";
import { humanStatus } from "./format";
import { useConversationIndexProgress } from "./useConversationIndexProgress";

function catalogQueryKey(
  filter: Filter,
  search: string,
  toolNames: string[],
  toolFailed: boolean,
): string {
  return JSON.stringify({
    search,
    toolNames,
    toolFailed,
    sources: filter.sources,
    projects: filter.projects,
    models: filter.models,
    providers: filter.providers,
    from: filter.from,
    to: filter.to,
  });
}

export function useConversationCatalog({
  filter,
  revision,
  onError,
}: {
  filter: Filter;
  revision: number;
  onError?: (error: unknown) => void;
}) {
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [toolNames, setToolNames] = useState<string[]>([]);
  const [toolFailed, setToolFailed] = useState(false);
  const [toolNameOptions, setToolNameOptions] = useState<string[]>([]);
  const [page, setPage] = useState(1);
  const [pageData, setPageData] = useState<ConversationPage>({ rows: [], total: 0 });
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const indexProgress = useConversationIndexProgress(revision);
  const catalogGeneration = useRef(0);
  const toolNamesGeneration = useRef(0);
  const queryKey = catalogQueryKey(filter, search, toolNames, toolFailed);
  const [seenQueryKey, setSeenQueryKey] = useState(queryKey);
  if (seenQueryKey !== queryKey) {
    setSeenQueryKey(queryKey);
    setPage(1);
  }
  const requestKey = `${queryKey}|${page}|${revision}`;
  const [seenRequestKey, setSeenRequestKey] = useState("");
  if (seenRequestKey !== requestKey) {
    setSeenRequestKey(requestKey);
    setLoading(true);
    setError(null);
  }

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(searchInput.trim()), 300);
    return () => window.clearTimeout(timer);
  }, [searchInput]);

  useEffect(() => {
    const generation = ++catalogGeneration.current;
    invoke<ConversationPage>("get_conversation_sessions_page", {
      query: {
        search: search || null,
        page,
        page_size: CATALOG_PAGE_SIZE,
        sources: filter.sources,
        projects: filter.projects,
        models: filter.models,
        providers: filter.providers,
        from: filter.from,
        to: filter.to,
        tool_names: toolNames,
        tool_failed: toolFailed,
      },
    })
      .then((result) => {
        if (generation === catalogGeneration.current) {
          setPageData(result);
        }
      })
      .catch((caught: unknown) => {
        if (generation === catalogGeneration.current) {
          setError(humanStatus(caught));
          onError?.(caught);
        }
      })
      .finally(() => {
        if (generation === catalogGeneration.current) {
          setLoading(false);
        }
      });
  }, [filter, revision, search, page, toolNames, toolFailed, onError]);

  useEffect(() => {
    const generation = ++toolNamesGeneration.current;
    invoke<string[]>("get_conversation_tool_names", {
      query: {
        sources: filter.sources,
        projects: filter.projects,
        models: filter.models,
        providers: filter.providers,
        from: filter.from,
        to: filter.to,
      },
    })
      .then((names) => {
        if (generation === toolNamesGeneration.current) {
          setToolNameOptions(names);
        }
      })
      .catch((caught: unknown) => {
        if (generation === toolNamesGeneration.current) {
          onError?.(caught);
        }
      });
  }, [filter, revision, onError]);

  return {
    searchInput,
    setSearchInput,
    search,
    setSearch,
    toolNames,
    setToolNames,
    toolFailed,
    setToolFailed,
    toolNameOptions,
    page,
    setPage,
    pageData,
    loading,
    error,
    indexProgress,
  };
}
