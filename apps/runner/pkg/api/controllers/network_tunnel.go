package controllers

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"strconv"

	common_proxy "github.com/boxlite-ai/common-go/pkg/proxy"
	"github.com/boxlite-ai/runner/pkg/runner"
	"github.com/gin-gonic/gin"
)

type guestTunnelTarget struct {
	boxID string
	dial  func() (net.Conn, error)
}

// BoxliteNetworkTunnel upgrades an authenticated CONNECT request to a raw guest stream.
func BoxliteNetworkTunnel(logger *slog.Logger, maxTunnels, maxPerBox int) gin.HandlerFunc {
	return networkTunnelHandler(logger, newTunnelLimits(maxTunnels, maxPerBox), func(ctx context.Context, boxID string, port uint16) (guestTunnelTarget, error) {
		r, err := runner.GetInstance(nil)
		if err != nil {
			return guestTunnelTarget{}, err
		}
		box, err := r.Boxlite.GetBox(ctx, boxID)
		if err != nil {
			return guestTunnelTarget{}, err
		}
		return guestTunnelTarget{
			boxID: box.ID(), // Names and IDs must spend the same Box's capacity.
			dial:  func() (net.Conn, error) { return r.Boxlite.DialGuestPort(ctx, box.ID(), port) },
		}, nil
	})
}

func networkTunnelHandler(logger *slog.Logger, limits *tunnelLimits, resolve func(context.Context, string, uint16) (guestTunnelTarget, error)) gin.HandlerFunc {
	return func(ctx *gin.Context) {
		if ctx.Request.Method != http.MethodConnect {
			ctx.JSON(http.StatusMethodNotAllowed, gin.H{"error": "CONNECT required"})
			return
		}
		boxID := ctx.Param("boxId")
		rawPort := ctx.Query("port")
		port, err := strconv.ParseUint(rawPort, 10, 16)
		if err != nil || port == 0 {
			ctx.JSON(http.StatusBadRequest, gin.H{"error": fmt.Sprintf("invalid target port %q", rawPort)})
			return
		}
		select {
		case limits.slots <- struct{}{}:
			defer func() { <-limits.slots }()
		default:
			ctx.JSON(http.StatusServiceUnavailable, gin.H{"error": common_proxy.ErrTunnelCapacity.Error()})
			return
		}
		target, err := resolve(ctx.Request.Context(), boxID, uint16(port))
		if err != nil {
			logger.WarnContext(ctx.Request.Context(), "guest tunnel lookup failed", "box", boxID, "error", err)
			ctx.JSON(http.StatusBadGateway, gin.H{"error": "guest tunnel unavailable"})
			return
		}
		if !limits.acquireBox(target.boxID) {
			ctx.JSON(http.StatusServiceUnavailable, gin.H{"error": common_proxy.ErrTunnelCapacity.Error()})
			return
		}
		defer limits.releaseBox(target.boxID)

		guestConn, err := target.dial()
		if err != nil {
			logger.WarnContext(ctx.Request.Context(), "guest tunnel dial failed", "box", boxID, "port", port, "error", err)
			ctx.JSON(http.StatusBadGateway, gin.H{"error": "guest tunnel unavailable"})
			return
		}

		clientConn, err := common_proxy.AcceptConnect(ctx.Writer)
		if err != nil {
			guestConn.Close()
			return
		}
		defer clientConn.Close()
		defer guestConn.Close()
		if err := common_proxy.ProxyBidirectionalStream(
			ctx.Request.Context(),
			clientConn,
			guestConn,
		); err != nil {
			logger.WarnContext(ctx.Request.Context(), "guest tunnel stream closed with error", "box", boxID, "port", port, "error", err)
		}
	}
}
