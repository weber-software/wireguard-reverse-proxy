wg-up:
	sudo wg-quick up ./wg0.conf

wg-down:
	sudo wg-quick down ./wg0.conf

nginx:
	docker run --net=host -it --rm nginx

ping:
	ping 192.168.222.11

curl:
	curl 192.168.222.11
