/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import { BadRequestException, Injectable, NotFoundException } from '@nestjs/common'
import { createCipheriv, createDecipheriv, createHash, randomBytes } from 'crypto'
import { BoxRepository, AdminBoxFilters } from '../../box/repositories/box.repository'
import { isBoxId } from '../../box/utils/box-id.util'
import { TypedConfigService } from '../../config/typed-config.service'
import { AdminBoxStatusDto } from '../dto/admin-box.dto'
import { AdminListBoxesQueryDto } from '../dto/admin-list-boxes-query.dto'
import { AdminPaginatedBoxesDto } from '../dto/admin-paginated-boxes.dto'

const CURSOR_VERSION = 1
const CURSOR_IV_BYTES = 12
const CURSOR_TAG_BYTES = 16

type CursorFilters = {
  state: AdminBoxFilters['state'] | null
  organizationId: string | null
  runnerId: string | null
  regionId: string | null
}

type CursorPayload = {
  version: typeof CURSOR_VERSION
  filters: CursorFilters
  updatedAt: string
  id: string
}

@Injectable()
export class AdminBoxReadService {
  private readonly cursorKey: Buffer

  constructor(
    private readonly boxRepository: BoxRepository,
    configService: TypedConfigService,
  ) {
    this.cursorKey = createHash('sha256').update(configService.getOrThrow('encryption.key')).digest()
  }

  async findAll(query: AdminListBoxesQueryDto): Promise<AdminPaginatedBoxesDto> {
    const filters = this.filtersFromQuery(query)
    const after = query.cursor ? this.decodeCursor(query.cursor, filters) : undefined
    const page = await this.boxRepository.findAdminPage({
      limit: query.limit,
      filters: this.repositoryFilters(filters),
      after,
    })
    const last = page.items.at(-1)

    return {
      items: page.items.map(AdminBoxStatusDto.fromRecord),
      nextCursor: page.hasMore && last ? this.encodeCursor(filters, last.cursorUpdatedAt, last.id) : null,
      fetchedAt: new Date().toISOString(),
    }
  }

  async findOne(boxId: string): Promise<AdminBoxStatusDto> {
    const box = await this.boxRepository.findAdminOne(boxId)
    if (!box) {
      throw new NotFoundException('Box not found')
    }
    return AdminBoxStatusDto.fromRecord(box)
  }

  private filtersFromQuery(query: AdminListBoxesQueryDto): CursorFilters {
    return {
      state: query.state ?? null,
      organizationId: query.organizationId ?? null,
      runnerId: query.runnerId ?? null,
      regionId: query.regionId ?? null,
    }
  }

  private repositoryFilters(filters: CursorFilters): AdminBoxFilters {
    return {
      state: filters.state ?? undefined,
      organizationId: filters.organizationId ?? undefined,
      runnerId: filters.runnerId ?? undefined,
      regionId: filters.regionId ?? undefined,
    }
  }

  private encodeCursor(filters: CursorFilters, updatedAt: string, id: string): string {
    const payload: CursorPayload = {
      version: CURSOR_VERSION,
      filters,
      updatedAt,
      id,
    }
    const iv = randomBytes(CURSOR_IV_BYTES)
    const cipher = createCipheriv('aes-256-gcm', this.cursorKey, iv)
    const ciphertext = Buffer.concat([cipher.update(JSON.stringify(payload), 'utf8'), cipher.final()])
    return Buffer.concat([iv, cipher.getAuthTag(), ciphertext]).toString('base64url')
  }

  private decodeCursor(cursor: string, filters: CursorFilters): { updatedAt: string; id: string } {
    try {
      const encoded = Buffer.from(cursor, 'base64url')
      if (encoded.length <= CURSOR_IV_BYTES + CURSOR_TAG_BYTES) {
        throw new Error('Cursor is too short')
      }
      const iv = encoded.subarray(0, CURSOR_IV_BYTES)
      const tag = encoded.subarray(CURSOR_IV_BYTES, CURSOR_IV_BYTES + CURSOR_TAG_BYTES)
      const decipher = createDecipheriv('aes-256-gcm', this.cursorKey, iv)
      decipher.setAuthTag(tag)
      const plaintext = Buffer.concat([
        decipher.update(encoded.subarray(CURSOR_IV_BYTES + CURSOR_TAG_BYTES)),
        decipher.final(),
      ]).toString('utf8')
      const payload = JSON.parse(plaintext) as unknown
      if (!this.isValidCursorPayload(payload, filters)) {
        throw new Error('Cursor payload is invalid')
      }
      return { updatedAt: payload.updatedAt, id: payload.id }
    } catch {
      throw new BadRequestException('Invalid cursor')
    }
  }

  private isValidCursorPayload(payload: unknown, filters: CursorFilters): payload is CursorPayload {
    if (!payload || typeof payload !== 'object') return false
    const candidate = payload as Partial<CursorPayload>
    if (candidate.version !== CURSOR_VERSION || !candidate.filters || typeof candidate.updatedAt !== 'string') {
      return false
    }
    if (typeof candidate.id !== 'string' || !isBoxId(candidate.id)) return false
    if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{6}Z$/.test(candidate.updatedAt)) return false
    if (Number.isNaN(new Date(candidate.updatedAt).valueOf())) return false
    return JSON.stringify(candidate.filters) === JSON.stringify(filters)
  }
}
