/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import { ApiProperty, ApiPropertyOptional, ApiSchema } from '@nestjs/swagger'
import { AdminBoxRecord } from '../../box/repositories/box.repository'
import { BoxDesiredState } from '../../box/enums/box-desired-state.enum'
import { BoxState } from '../../box/enums/box-state.enum'

@ApiSchema({ name: 'AdminBoxResources' })
export class AdminBoxResourcesDto {
  @ApiProperty({ example: 2 })
  cpu: number

  @ApiProperty({ example: 4 })
  memoryGiB: number

  @ApiProperty({ example: 20 })
  diskGiB: number
}

@ApiSchema({ name: 'AdminBoxStatus' })
export class AdminBoxStatusDto {
  @ApiProperty({ example: 'Ab3xYz09LmN2' })
  id: string

  @ApiProperty({ example: 'box-name' })
  name: string

  @ApiProperty({ format: 'uuid' })
  organizationId: string

  @ApiPropertyOptional({ format: 'uuid', nullable: true })
  runnerId: string | null

  @ApiProperty({ example: 'us' })
  regionId: string

  @ApiProperty({ enum: BoxState, enumName: 'BoxState' })
  state: BoxState

  @ApiProperty({ enum: BoxDesiredState, enumName: 'BoxDesiredState' })
  desiredState: BoxDesiredState

  @ApiProperty({ type: AdminBoxResourcesDto })
  resources: AdminBoxResourcesDto

  @ApiPropertyOptional({ enum: ['recoverable', 'unrecoverable'], nullable: true })
  errorCategory: 'recoverable' | 'unrecoverable' | null

  @ApiProperty({ format: 'date-time' })
  createdAt: string

  @ApiProperty({ format: 'date-time' })
  updatedAt: string

  static fromRecord(box: AdminBoxRecord): AdminBoxStatusDto {
    return {
      id: box.id,
      name: box.name,
      organizationId: box.organizationId,
      runnerId: box.runnerId ?? null,
      regionId: box.region,
      state: box.state,
      desiredState: box.desiredState,
      resources: { cpu: box.cpu, memoryGiB: box.mem, diskGiB: box.disk },
      errorCategory: box.state === BoxState.ERROR ? (box.recoverable ? 'recoverable' : 'unrecoverable') : null,
      createdAt: new Date(box.createdAt).toISOString(),
      updatedAt: new Date(box.updatedAt).toISOString(),
    }
  }
}
