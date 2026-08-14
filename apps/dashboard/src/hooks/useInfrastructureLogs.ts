/*
 * Copyright Daytona Platforms Inc.
 * SPDX-License-Identifier: AGPL-3.0
 */

import { useApi } from '@/hooks/useApi'
import {
  AdminSearchInfrastructureLogsSourceEnum,
  AdminSearchPlatformLogsSourceEnum,
  LogEntry,
} from '@boxlite-ai/api-client'
import { useQuery } from '@tanstack/react-query'

export type InfrastructureLogSource = 'runner' | 'collector'
export type PlatformLogSource = 'api' | 'worker' | 'runner' | 'box'

export interface InfrastructureLogsQuery {
  source: InfrastructureLogSource
  from: Date
  to: Date
  search?: string
  limit: number
  nextToken?: string
}

export interface InfrastructureLogsPage {
  items: LogEntry[]
  nextToken?: string
}

export interface PlatformLogsQuery {
  source: PlatformLogSource
  from: Date
  to: Date
  page: number
  limit: number
  boxId?: string
  severities?: string[]
  search?: string
  traceId?: string
}

export interface PlatformLogsPage {
  items: LogEntry[]
  total: number
  page: number
  totalPages: number
}

export function useInfrastructureLogsAccess() {
  const api = useApi()
  return useQuery({
    queryKey: ['infrastructure-logs', 'access'],
    queryFn: async () => {
      const response = await api.adminApi.adminCheckInfrastructureLogsAccess({ timeout: 10_000 })
      return response.data
    },
    staleTime: 5 * 60_000,
    retry: false,
  })
}

export function useInfrastructureLogs(query: InfrastructureLogsQuery) {
  const api = useApi()
  return useQuery({
    queryKey: ['infrastructure-logs', query],
    queryFn: async () => {
      const response = await api.adminApi.adminSearchInfrastructureLogs(
        query.from,
        query.to,
        query.source as AdminSearchInfrastructureLogsSourceEnum,
        query.search,
        query.limit,
        query.nextToken,
        { timeout: 10_000 },
      )
      return response.data
    },
    staleTime: 10_000,
  })
}

export function usePlatformLogs(query: PlatformLogsQuery, enabled = true) {
  const api = useApi()
  return useQuery({
    queryKey: ['platform-logs', query],
    queryFn: async () => {
      const response = await api.adminApi.adminSearchPlatformLogs(
        query.from,
        query.to,
        query.page,
        query.limit,
        query.severities,
        query.search,
        query.source as AdminSearchPlatformLogsSourceEnum,
        query.boxId,
        query.traceId,
        { timeout: 10_000 },
      )
      return response.data
    },
    enabled,
    staleTime: 10_000,
  })
}
