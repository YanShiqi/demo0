#!/usr/bin/env bash
set -euo pipefail

APP_NAME="demo0"
SHUTDOWN_WAIT_SECONDS=10

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BINARY_PATH="${REPO_ROOT}/target/debug/${APP_NAME}"
CANONICAL_BINARY_PATH="$(readlink -f "${BINARY_PATH}" 2>/dev/null || echo "${BINARY_PATH}")"
RUN_DIR="${REPO_ROOT}/data/run"
LOG_DIR="${REPO_ROOT}/data/logs"
PID_FILE="${RUN_DIR}/${APP_NAME}.pid"
LOG_FILE="${LOG_DIR}/${APP_NAME}.log"
APP_PORT_VALUE=""

usage() {
    echo "用法: $0 {start|stop|restart|status|logs}"
}

ensure_runtime_dirs() {
    mkdir -p "${RUN_DIR}" "${LOG_DIR}"
}

read_pid() {
    if [[ -f "${PID_FILE}" ]]; then
        tr -d '[:space:]' < "${PID_FILE}"
    fi
}

is_running() {
    local pid="${1:-}"
    [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null
}

is_expected_process() {
    local pid="${1:-}"
    local command_line
    local executable_path
    [[ -n "${pid}" ]] || return 1
    executable_path="$(readlink -f "/proc/${pid}/exe" 2>/dev/null || true)"
    if [[ "${executable_path}" == "${CANONICAL_BINARY_PATH}" ]]; then
        return 0
    fi
    command_line="$(ps -p "${pid}" -o args= 2>/dev/null || true)"
    [[ "${command_line}" == *"${BINARY_PATH}"* || "${command_line}" == *"${CANONICAL_BINARY_PATH}"* ]]
}

configured_port() {
    local env_port
    local toml_port
    if [[ -f "${REPO_ROOT}/.env" ]]; then
        env_port="$(awk -F '=' '
            /^[[:space:]]*APP_PORT[[:space:]]*=/ {
                value = $2
                gsub(/[[:space:]]/, "", value)
                gsub(/"/, "", value)
                print value
            }
        ' "${REPO_ROOT}/.env" | tail -n 1)"
        if [[ -n "${env_port}" ]]; then
            echo "${env_port}"
            return
        fi
    fi

    toml_port="$(awk -F '=' '
        /^[[:space:]]*port[[:space:]]*=/ {
            value = $2
            gsub(/[[:space:]]/, "", value)
            gsub(/"/, "", value)
            print value
            exit
        }
    ' "${REPO_ROOT}/config/default.toml")"
    echo "${toml_port:-6324}"
}

find_expected_pid_by_port() {
    local pid
    [[ -n "${APP_PORT_VALUE}" ]] || return 1
    command -v lsof >/dev/null 2>&1 || return 1
    while read -r pid; do
        if is_running "${pid}" && is_expected_process "${pid}"; then
            echo "${pid}"
            return 0
        fi
    done < <(lsof -tiTCP:"${APP_PORT_VALUE}" -sTCP:LISTEN 2>/dev/null || true)
    return 1
}

find_running_server_pid() {
    local pid
    pid="$(read_pid)"
    if is_running "${pid}" && is_expected_process "${pid}"; then
        echo "${pid}"
        return 0
    fi

    pid="$(find_expected_pid_by_port || true)"
    if [[ -n "${pid}" ]]; then
        echo "${pid}" > "${PID_FILE}"
        echo "${pid}"
        return 0
    fi
    return 1
}

start_server() {
    ensure_runtime_dirs
    local pid
    pid="$(find_running_server_pid || true)"
    if [[ -n "${pid}" ]]; then
        echo "服务器已在运行，PID=${pid}"
        echo "日志: ${LOG_FILE}"
        return
    fi

    if [[ -n "${pid}" && -f "${PID_FILE}" ]]; then
        echo "发现过期 PID 文件，已忽略: ${PID_FILE}"
    fi

    echo "正在构建 ${APP_NAME}..."
    (cd "${REPO_ROOT}" && cargo build)

    echo "正在后台启动服务器..."
    # 使用 nohup 让服务脱离当前终端；真实运行配置仍由 config/default.toml 和 .env 决定。
    (cd "${REPO_ROOT}" && nohup "${BINARY_PATH}" >> "${LOG_FILE}" 2>&1 & echo $! > "${PID_FILE}")

    pid="$(read_pid)"
    sleep 1
    if is_running "${pid}" && is_expected_process "${pid}"; then
        echo "服务器已启动，PID=${pid}"
        echo "日志: ${LOG_FILE}"
    else
        rm -f "${PID_FILE}"
        pid="$(find_running_server_pid || true)"
        if [[ -n "${pid}" ]]; then
            echo "服务器已启动，PID=${pid}"
            echo "日志: ${LOG_FILE}"
            return
        fi
        echo "服务器启动失败，请查看日志: ${LOG_FILE}" >&2
        return 1
    fi
}

stop_server() {
    local pid
    pid="$(find_running_server_pid || true)"
    if ! is_running "${pid}"; then
        echo "服务器未运行"
        rm -f "${PID_FILE}"
        return
    fi
    if ! is_expected_process "${pid}"; then
        echo "PID 文件指向的不是当前项目服务器，拒绝停止: PID=${pid}" >&2
        return 1
    fi

    echo "正在停止服务器，PID=${pid}..."
    kill "${pid}"
    for _ in $(seq 1 "${SHUTDOWN_WAIT_SECONDS}"); do
        if ! is_running "${pid}"; then
            rm -f "${PID_FILE}"
            echo "服务器已停止"
            return
        fi
        sleep 1
    done

    echo "服务器未在 ${SHUTDOWN_WAIT_SECONDS} 秒内退出，请检查进程 PID=${pid}" >&2
    return 1
}

status_server() {
    local pid
    pid="$(find_running_server_pid || true)"
    if [[ -n "${pid}" ]] && is_running "${pid}" && is_expected_process "${pid}"; then
        echo "服务器运行中，PID=${pid}"
        echo "日志: ${LOG_FILE}"
    else
        rm -f "${PID_FILE}"
        echo "服务器未运行"
    fi
}

show_logs() {
    ensure_runtime_dirs
    touch "${LOG_FILE}"
    tail -f "${LOG_FILE}"
}

main() {
    local command="${1:-}"
    APP_PORT_VALUE="$(configured_port)"
    case "${command}" in
        start)
            start_server
            ;;
        stop)
            stop_server
            ;;
        restart)
            stop_server || true
            start_server
            ;;
        status)
            status_server
            ;;
        logs)
            show_logs
            ;;
        *)
            usage
            return 1
            ;;
    esac
}

main "$@"
