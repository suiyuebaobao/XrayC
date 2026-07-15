{
    admin off
}

__PANEL__ {
    encode zstd gzip

    handle /api/deploy/artifacts/* {
        reverse_proxy __UPSTREAM__ {
            transport http {
                dial_timeout 60s
                response_header_timeout 900s
            }
        }
    }

    handle /api/* {
        reverse_proxy __UPSTREAM__
    }

    handle /sub/* {
        reverse_proxy __UPSTREAM__
    }

    handle /health {
        reverse_proxy __UPSTREAM__
    }

    handle {
        root * /srv/xrayc/frontend
        try_files {path} /index.html
        file_server
    }
}
