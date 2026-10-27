import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { ApiError, useBootstrap } from '@/lib/api'

// One connection notification for the workspace; no permanent header control.
export default function ConnectionStatus() {
  const bootstrap = useBootstrap()
  const client = useQueryClient()
  const navigate = useNavigate()
  useEffect(() => {
    const id = 'service-connection'
    if (!bootstrap.error) {
      toast.dismiss(id)
      return
    }
    const unavailable = bootstrap.error instanceof ApiError && bootstrap.error.code === 'unavailable'
    toast.error(unavailable ? '无法连接词典服务' : bootstrap.error.message, {
      id,
      duration: Infinity,
      action: { label: '重试', onClick: () => { void client.refetchQueries({ type: 'active' }) } },
      cancel: { label: '连接设置', onClick: () => navigate('/settings/security') },
    })
  }, [bootstrap.error, client, navigate])
  return null
}
