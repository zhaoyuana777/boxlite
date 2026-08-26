/*
 * Copyright 2025 Daytona Platforms Inc.
 * Modified by BoxLite AI, 2025-2026
 * SPDX-License-Identifier: AGPL-3.0
 */

import { ApiProperty, ApiPropertyOptional, ApiSchema } from '@nestjs/swagger'
import { AdminBoxStatusDto } from './admin-box.dto'

@ApiSchema({ name: 'AdminPaginatedBoxes' })
export class AdminPaginatedBoxesDto {
  @ApiProperty({ type: [AdminBoxStatusDto] })
  items: AdminBoxStatusDto[]

  @ApiPropertyOptional({ nullable: true })
  nextCursor: string | null

  @ApiProperty({ format: 'date-time' })
  fetchedAt: string
}
