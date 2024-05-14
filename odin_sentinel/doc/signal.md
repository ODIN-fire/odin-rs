# Signal Alarm Server

## Setting up `signal-cli`

### install `signal-cli` on reachable edge-server

```
git clone https://github.com/AsamK/signal-cli
cd signal-cli
./gradlew build
./gradlew fatJar
```
this will create a `build/libs/signal-cli-fat-<version>-SNAPSHOT.jar` which then can be executed with
```
java -jar build/libs/signal-cli-fat-<version>-SNAPSHOT.jar <args>...
```

or install as binary package for respective OS, e.g. on macOS:
```
brew install signal-cli
```

### register account

(a) obtain captcha

from https://github.com/AsamK/signal-cli/blob/master/man/signal-cli.1.adoc

goto https://signalcaptchas.org/registration/generate.html

After solving the captcha, right-click on the "Open Signal" link and copy the link.

signalcaptcha://signal-hcaptcha.5fad97ac-7d06-4e44-b18a-....

(b) register account on edge server device

run 
```
   signal-cli -a "+1<account-phone>" -v register --captcha "...."
```
sends sms with 6 digit verify-code

run
```
   signal-cli -a "+1<account-phone>" -v verify <verify-code>
```

***WATCH OUT*** this invalidates account on previous device (e.g. phone)

alternative you can run as linked device

### link device

install `qrencode`

run
```
signal-cli link -n `hostname` > .signal-link.txt&
sleep 5
qrencode -t ANSI `cat .signal-link.txt`
```
then use phone Signal > Settings > linked devices to add new link scanning QR code 
(make sure that `~/.local/share/signal-cli/data` is empty)


### run server

signal-cli -a "+1<account-phone >" -v daemon --http localhost:9009


(or `java -jar <path-to-fat-jar> <args>...` - see above)


### open problems

if recipient is account this is a note-to-self message that does (on Android) not trigger a notification
